//! Compiled rules and evaluation (port of `ast.go` `lowerAST`, `nodes.go`,
//! `values.go`, `functions.go`, and `trace.go`).
//!
//! Evaluation is generic over a trace slot (`()` or `Option<Frag>`). `Rule::eval` branches on
//! the trace flag once; the untraced instantiation contains no trace code.

mod compare;
#[doc(hidden)]
pub use compare::cmp_number;
pub(crate) mod trace;

use smallvec::SmallVec;

use crate::ast::{Ast, AstKind, LiteralKind, NodeData, NodeId, Operator, Segment};
use crate::env::Env;
use crate::error::{Error, ParseError};
use crate::func::CallFailure;
use crate::input::Input;
use crate::literal::parse_literal;
use crate::print::{canonical_all, path_string};
use crate::value::{ArrayRef, Val, Value, ValueRef};
use compare::{CmpOp, compare, compare_slice};
use trace::{Diagnostic, Frag, Trace, combine};

/// A compiled expression node. `meta` identifies the AST node for traces; it
/// is `None` only for the base operator under a negated `not in`/`not
/// contains`/`not matches`, which Go traces as one node.
#[derive(Clone, Debug)]
pub(crate) struct Node {
    kind: Kind,
    meta: Option<Meta>,
}

#[derive(Clone, Debug)]
pub(crate) struct Meta {
    pub id: NodeId,
    pub kind: AstKind,
    /// Canonical expression of the node.
    pub expr: Box<str>,
}

#[derive(Clone, Debug)]
enum Kind {
    And(Box<Node>, Box<Node>),
    Or(Box<Node>, Box<Node>),
    Not(Box<Node>),
    Compare {
        op: CmpOp,
        lhs: Box<Node>,
        rhs: Box<Node>,
    },
    Match {
        lhs: Box<Node>,
        rhs: Box<Node>,
    },
    In {
        lhs: Box<Node>,
        rhs: Box<Node>,
    },
    Literal(Value),
    Path {
        segments: Box<[Segment]>,
        text: Box<str>,
    },
    Array(Box<[Node]>),
    /// An array whose items are all literals, built once. The item nodes are
    /// kept for traces.
    ConstArray(Box<[Value]>, Box<[Node]>),
    Call {
        target: Call,
        args: Box<[Node]>,
    },
}

#[derive(Clone, Debug)]
enum Call {
    StartsWith,
    Named(Box<str>),
}

/// Lower an AST node (Go `lowerAST`). Literal values are parsed here; the
/// first invalid literal, in evaluation order, is the error.
pub(crate) fn lower(ast: &Ast, id: NodeId) -> Result<Node, ParseError> {
    lower_node(ast, &mut canonical_all(ast), id)
}

/// Lower one node; `texts` holds each node's canonical expression (taken by
/// the node's trace metadata).
fn lower_node(ast: &Ast, texts: &mut [String], id: NodeId) -> Result<Node, ParseError> {
    let kind = match ast.data(id) {
        NodeData::Literal { span, kind } => Kind::Literal(
            parse_literal(*kind, ast.text(*span))
                .map_err(|err| ParseError::at(ast.source(), span.start, err))?,
        ),
        NodeData::Path { segments, .. } => Kind::Path {
            segments: segments.clone(),
            text: path_string(segments).into_boxed_str(),
        },
        NodeData::Array { items, .. } => {
            let items = items
                .iter()
                .map(|&item| lower_node(ast, texts, item))
                .collect::<Result<Vec<_>, _>>()?;
            if items
                .iter()
                .all(|item| matches!(item.kind, Kind::Literal(_)))
            {
                let values = items.iter().map(|item| match &item.kind {
                    Kind::Literal(v) => v.clone(),
                    _ => unreachable!("checked above"),
                });
                Kind::ConstArray(values.collect(), items.into_boxed_slice())
            } else {
                Kind::Array(items.into_boxed_slice())
            }
        }
        NodeData::Call { name, args, .. } => {
            let args = args
                .iter()
                .map(|&arg| lower_node(ast, texts, arg))
                .collect::<Result<Vec<_>, _>>()?;
            let name = ast.text(*name);
            let target = if name == "starts_with" {
                Call::StartsWith
            } else {
                Call::Named(name.into())
            };
            Kind::Call {
                target,
                args: args.into_boxed_slice(),
            }
        }
        NodeData::Unary { operand, .. } => Kind::Not(Box::new(lower_node(ast, texts, *operand)?)),
        NodeData::Binary {
            op,
            negated,
            lhs,
            rhs,
            ..
        } => {
            let rhs_is_cidr = matches!(
                ast.data(*rhs),
                NodeData::Literal {
                    kind: LiteralKind::Cidr,
                    ..
                }
            );
            let lhs = Box::new(lower_node(ast, texts, *lhs)?);
            let rhs = Box::new(lower_node(ast, texts, *rhs)?);
            let cmp = |op| match op {
                Operator::Eq => Some(CmpOp::Eq),
                Operator::Ne => Some(CmpOp::Ne),
                Operator::Gt => Some(CmpOp::Gt),
                Operator::Ge => Some(CmpOp::Ge),
                Operator::Lt => Some(CmpOp::Lt),
                Operator::Le => Some(CmpOp::Le),
                Operator::Contains => Some(CmpOp::Contains),
                // `x in <CIDR>` is CIDR containment, i.e. `x == <CIDR>`.
                Operator::In if rhs_is_cidr => Some(CmpOp::Eq),
                _ => None,
            };
            let base = match (op, cmp(*op)) {
                (_, Some(op)) => Kind::Compare { op, lhs, rhs },
                (Operator::And, _) => Kind::And(lhs, rhs),
                (Operator::Or, _) => Kind::Or(lhs, rhs),
                (Operator::Matches, _) => Kind::Match { lhs, rhs },
                (Operator::In, _) => Kind::In { lhs, rhs },
                _ => unreachable!("not is unary; comparisons are handled above"),
            };
            if *negated {
                Kind::Not(Box::new(Node {
                    kind: base,
                    meta: None,
                }))
            } else {
                base
            }
        }
    };
    let meta = Meta {
        id,
        kind: ast.node(id).kind(),
        expr: std::mem::take(&mut texts[id.index()]).into_boxed_str(),
    };
    Ok(Node {
        kind,
        meta: Some(meta),
    })
}

/// Missing field names, borrowed from compiled rules.
pub(crate) type Missing<'a> = SmallVec<[&'a str; 2]>;

/// The rare parts of a result, boxed so results stay small on the happy path:
/// an error, and missing field names a function reported (owned).
#[derive(Debug, Default)]
pub(crate) struct Problem {
    pub(crate) error: Option<Error>,
    pub(crate) missing: Vec<String>,
}

/// The outcome of [`Rule::eval`](crate::Rule::eval).
///
/// Exactly one of these holds:
/// - [`error`](Self::error) is set: evaluation failed (an input or function
///   error, an unknown function, a bad argument);
/// - [`unknown`](Self::unknown): no error, but the input lacked
///   [`missing_fields`](Self::missing_fields) needed to decide;
/// - [`complete`](Self::complete): the rule produced [`value`](Self::value),
///   which [`pass`](Self::pass)es if non-zero and [`fail`](Self::fail)s
///   otherwise.
///
/// ```rust
/// use rulekit::{NoInput, Opts};
///
/// let rule = rulekit::parse("port == 443 and tls")?;
/// let result = rule.eval(&(), &NoInput, Opts::default());
/// assert!(result.unknown());
/// assert_eq!(result.missing_fields().collect::<Vec<_>>(), ["port", "tls"]);
/// # Ok::<(), rulekit::ParseError>(())
/// ```
#[derive(Debug)]
pub struct EvalResult<'a>(Res<'a, Option<Frag<'a>>>);

impl<'a> EvalResult<'a> {
    /// The result value; `Null` when the rule did not produce one.
    pub fn value(&self) -> ValueRef<'_> {
        self.0.value.as_ref()
    }

    /// The result value, owned or borrowed from the rule, input, or context.
    pub fn into_value(self) -> Val<'a> {
        self.0.value
    }

    /// The evaluation error, if any. When both sides of `and`/`or` fail,
    /// this is an [`Error::Multiple`].
    pub fn error(&self) -> Option<&Error> {
        self.0.error()
    }

    /// Fields the rule needed but the input lacked: paths in the rule, then
    /// any names a custom function reported.
    pub fn missing_fields(&self) -> impl Iterator<Item = &str> + '_ {
        self.0.missing_fields()
    }

    /// The evaluation trace, when tracing was enabled.
    pub fn trace(&self) -> Option<&Trace<'a>> {
        self.0.trace.as_ref().and_then(Frag::node)
    }

    /// No error and no missing fields.
    pub fn complete(&self) -> bool {
        self.0.complete()
    }

    /// Same as [`complete`](Self::complete).
    pub fn ok(&self) -> bool {
        self.0.complete()
    }

    /// No error, but more input is needed.
    pub fn unknown(&self) -> bool {
        self.0.error().is_none() && !self.0.complete()
    }

    /// Complete with a non-zero value.
    pub fn pass(&self) -> bool {
        self.0.pass()
    }

    /// Complete with a zero value.
    pub fn fail(&self) -> bool {
        self.0.fail()
    }
}

/// Where a node's evaluation keeps its trace fragment: `()` when not
/// tracing (the slot does not exist, so untraced results stay small), or
/// `Option<Frag>` when tracing.
pub(crate) trait Slot<'a>: Sized {
    const ENABLED: bool;
    fn empty() -> Self;
    fn take_frag(&mut self) -> Option<Frag<'a>>;
    fn put_frag(&mut self, frag: Option<Frag<'a>>);
}

impl<'a> Slot<'a> for () {
    const ENABLED: bool = false;
    fn empty() -> Self {}
    fn take_frag(&mut self) -> Option<Frag<'a>> {
        None
    }
    fn put_frag(&mut self, _: Option<Frag<'a>>) {}
}

impl<'a> Slot<'a> for Option<Frag<'a>> {
    const ENABLED: bool = true;
    fn empty() -> Self {
        None
    }
    fn take_frag(&mut self) -> Option<Frag<'a>> {
        self.take()
    }
    fn put_frag(&mut self, frag: Option<Frag<'a>>) {
        *self = frag;
    }
}

/// A node's evaluation result (Go `Result`), with a trace slot `S`.
#[derive(Debug)]
pub(crate) struct Res<'a, S> {
    pub(crate) value: Val<'a>,
    pub(crate) problem: Option<Box<Problem>>,
    pub(crate) missing: Missing<'a>,
    trace: S,
}

impl<'a, S: Slot<'a>> Res<'a, S> {
    fn of(value: Val<'a>) -> Self {
        Res {
            value,
            problem: None,
            missing: Missing::new(),
            trace: S::empty(),
        }
    }

    fn bool(b: bool) -> Self {
        Self::of(Val::Ref(ValueRef::Bool(b)))
    }

    fn failed(error: Error) -> Self {
        Res {
            problem: Some(Box::new(Problem {
                error: Some(error),
                missing: Vec::new(),
            })),
            ..Self::of(Val::Ref(ValueRef::Null))
        }
    }

    /// Keep only the error and missing fields (Go returns these without a
    /// value), with the given trace.
    fn incomplete(self, trace: Option<Frag<'a>>) -> Self {
        let mut out = Res {
            value: Val::Ref(ValueRef::Null),
            problem: self.problem,
            missing: self.missing,
            trace: S::empty(),
        };
        out.trace.put_frag(trace);
        out
    }

    fn with_trace(mut self, trace: Option<Frag<'a>>) -> Self {
        self.trace.put_frag(trace);
        self
    }

    /// The result as returned to callers, with no trace.
    fn untraced(self) -> EvalResult<'a> {
        EvalResult(Res {
            value: self.value,
            problem: self.problem,
            missing: self.missing,
            trace: None,
        })
    }

    pub(crate) fn complete(&self) -> bool {
        self.problem.is_none() && self.missing.is_empty()
    }

    pub(crate) fn error(&self) -> Option<&Error> {
        self.problem.as_deref().and_then(|p| p.error.as_ref())
    }

    /// Missing field names: from paths, then any a function reported.
    pub(crate) fn missing_fields(&self) -> impl Iterator<Item = &str> + '_ {
        let reported = self.problem.as_deref().map_or(&[][..], |p| &p.missing[..]);
        self.missing
            .iter()
            .copied()
            .chain(reported.iter().map(String::as_str))
    }

    fn ok(&self) -> bool {
        self.complete()
    }

    pub(crate) fn pass(&self) -> bool {
        self.complete() && !self.value.as_ref().is_zero()
    }

    fn fail(&self) -> bool {
        self.complete() && self.value.as_ref().is_zero()
    }
}

/// Evaluate a rule's root node, with or without a trace.
pub(crate) fn run<'a, C: ?Sized, I: Input<C> + ?Sized>(
    root: &'a Node,
    scope: &Scope<'a, C, I>,
    trace: bool,
) -> EvalResult<'a> {
    if trace {
        EvalResult(root.eval::<Option<Frag<'a>>, C, I>(scope))
    } else {
        root.eval::<(), C, I>(scope).untraced()
    }
}

/// Go `unionUnique`.
fn union<'a>(left: Missing<'a>, right: Missing<'a>) -> Missing<'a> {
    if left.is_empty() {
        return right;
    }
    let mut out = left;
    for name in right {
        if !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

/// Go `coalesceErrs` for errors, and `unionUnique` for reported missing
/// field names.
fn coalesce(left: Option<Box<Problem>>, right: Option<Box<Problem>>) -> Option<Box<Problem>> {
    match (left, right) {
        (Some(mut l), Some(r)) => {
            let r = *r;
            l.error = match (l.error.take(), r.error) {
                (Some(a), Some(b)) => Some(Error::Multiple(vec![a, b])),
                (a, b) => a.or(b),
            };
            for name in r.missing {
                if !l.missing.contains(&name) {
                    l.missing.push(name);
                }
            }
            Some(l)
        }
        (l, r) => l.or(r),
    }
}

/// Everything a node needs while evaluating.
pub(crate) struct Scope<'a, C: ?Sized, I: ?Sized> {
    pub input: &'a I,
    pub ctx: &'a C,
    pub env: &'a Env<C>,
}

type Buf<'a, const N: usize> = SmallVec<[Val<'a>; N]>;

impl Node {
    /// Evaluate the node; when tracing, describe it (Go `tracedRule`).
    pub(crate) fn eval<'a, S: Slot<'a>, C: ?Sized, I: Input<C> + ?Sized>(
        &'a self,
        s: &Scope<'a, C, I>,
    ) -> Res<'a, S> {
        let mut r = self.eval_kind::<S, C, I>(s);
        if S::ENABLED
            && let Some(meta) = &self.meta
        {
            let inner = r.trace.take_frag();
            r.trace.put_frag(Some(trace::wrap(meta, &r, inner)));
        }
        r
    }

    /// Go `prunedTrace`.
    fn pruned(&self) -> Option<Frag<'_>> {
        self.meta.as_ref().map(trace::pruned)
    }

    fn eval_kind<'a, S: Slot<'a>, C: ?Sized, I: Input<C> + ?Sized>(
        &'a self,
        s: &Scope<'a, C, I>,
    ) -> Res<'a, S> {
        match &self.kind {
            Kind::And(l, r) => {
                let mut left = l.eval::<S, C, I>(s);
                if left.fail() {
                    if S::ENABLED {
                        let frag = combine([left.trace.take_frag(), r.pruned()]);
                        left.trace.put_frag(frag);
                    }
                    return left;
                }
                let mut right = r.eval::<S, C, I>(s);
                if right.fail() {
                    if S::ENABLED {
                        let frag = combine([left.trace.take_frag(), right.trace.take_frag()]);
                        right.trace.put_frag(frag);
                    }
                    return right;
                }
                merge::<S>(left, right)
            }
            Kind::Or(l, r) => {
                let mut left = l.eval::<S, C, I>(s);
                if left.pass() {
                    if S::ENABLED {
                        let frag = combine([left.trace.take_frag(), r.pruned()]);
                        left.trace.put_frag(frag);
                    }
                    return left;
                }
                let mut right = r.eval::<S, C, I>(s);
                if right.pass() {
                    if S::ENABLED {
                        let frag = combine([left.trace.take_frag(), right.trace.take_frag()]);
                        right.trace.put_frag(frag);
                    }
                    return right;
                }
                merge::<S>(left, right)
            }
            Kind::Not(inner) => {
                let mut r = inner.eval::<S, C, I>(s);
                // A negated operator has no traced node between it and its
                // operands, so its operand traces become its children.
                let trace = if S::ENABLED {
                    match r.trace.take_frag() {
                        Some(t) if t.is_group() => Some(t),
                        other => combine([other]),
                    }
                } else {
                    None
                };
                if !r.ok() {
                    return r.incomplete(trace);
                }
                Res::bool(r.value.as_ref().is_zero()).with_trace(trace)
            }
            Kind::Compare { op, lhs, rhs } => self.binary::<S, C, I>(lhs, rhs, s, |lv, rv| {
                let outcome = compare(lv, *op, rv);
                (
                    outcome.pass,
                    if S::ENABLED {
                        Diagnostic::new(outcome.diagnostic, lv, *op, rv)
                    } else {
                        None
                    },
                )
            }),
            Kind::Match { lhs, rhs } => {
                self.binary::<S, C, I>(lhs, rhs, s, |lv, rv| (matches(lv, rv), None))
            }
            Kind::In { lhs, rhs } => self.binary::<S, C, I>(lhs, rhs, s, |lv, rv| {
                // The parser guarantees a list on the right.
                let ValueRef::Array(_) = rv else {
                    return (false, None);
                };
                // `x in list` is `list contains x`; a list-valued x is in the
                // list when ANY of its elements is.
                let outcome = match lv {
                    ValueRef::Array(items) => {
                        compare_slice(items, CmpOp::Eq, |el, _| compare(rv, CmpOp::Contains, el))
                    }
                    _ => compare(rv, CmpOp::Contains, lv),
                };
                let diagnostic = if S::ENABLED {
                    Diagnostic::new(outcome.diagnostic, rv, CmpOp::Contains, lv)
                } else {
                    None
                };
                (outcome.pass, diagnostic)
            }),
            Kind::Literal(v) => Res::of(Val::Ref(v.as_ref())),
            Kind::ConstArray(values, items) => {
                if S::ENABLED {
                    return eval_array::<S, C, I>(items, s);
                }
                Res::of(Val::Ref(ValueRef::Array(ArrayRef::Values(values))))
            }
            Kind::Path { segments, text } => match s.input.get(s.ctx, segments) {
                Ok(Some(v)) => Res::of(v),
                Ok(None) => {
                    let mut missing = Missing::new();
                    missing.push(&**text);
                    Res {
                        missing,
                        ..Res::of(Val::Ref(ValueRef::Null))
                    }
                }
                Err(source) => Res::failed(Error::Input {
                    field: text.to_string(),
                    source,
                }),
            },
            Kind::Array(items) => eval_array::<S, C, I>(items, s),
            Kind::Call { target, args } => call::<S, C, I>(target, args, s),
        }
    }

    /// Go `nodeCompare`/`nodeMatch`/`nodeIn`: evaluate both operands (a
    /// non-ok left operand prunes the right), then apply `f`, which returns
    /// the result and, when tracing, a diagnostic.
    fn binary<'a, S: Slot<'a>, C: ?Sized, I: Input<C> + ?Sized>(
        &'a self,
        lhs: &'a Node,
        rhs: &'a Node,
        s: &Scope<'a, C, I>,
        f: impl FnOnce(ValueRef<'_>, ValueRef<'_>) -> (bool, Option<Diagnostic>),
    ) -> Res<'a, S> {
        let (mut lbuf, mut rbuf) = (Buf::<8>::new(), Buf::<8>::new());
        let (mut left, left_in_buf) = lhs.operand::<S, C, I, 8>(s, &mut lbuf);
        if !left.ok() {
            let trace = if S::ENABLED {
                combine([left.trace.take_frag(), rhs.pruned()])
            } else {
                None
            };
            return left.incomplete(trace);
        }
        let (mut right, right_in_buf) = rhs.operand::<S, C, I, 8>(s, &mut rbuf);
        let trace = if S::ENABLED {
            combine([left.trace.take_frag(), right.trace.take_frag()])
        } else {
            None
        };
        if !right.ok() {
            return right.incomplete(trace);
        }
        let lv = if left_in_buf {
            ValueRef::Array(ArrayRef::Vals(&lbuf))
        } else {
            left.value.as_ref()
        };
        let rv = if right_in_buf {
            ValueRef::Array(ArrayRef::Vals(&rbuf))
        } else {
            right.value.as_ref()
        };
        let (pass, diagnostic) = f(lv, rv);
        let mut trace = trace;
        if let Some(diagnostic) = diagnostic {
            trace = trace::add_diagnostic(trace, diagnostic);
        }
        Res::bool(pass).with_trace(trace)
    }

    /// Evaluate a comparison operand. Untraced, a non-constant array literal
    /// is evaluated into `buf` (the bool is true) so comparing against it does
    /// not allocate; traced, it is evaluated as a node like in Go.
    fn operand<'a, S: Slot<'a>, C: ?Sized, I: Input<C> + ?Sized, const N: usize>(
        &'a self,
        s: &Scope<'a, C, I>,
        buf: &mut Buf<'a, N>,
    ) -> (Res<'a, S>, bool) {
        if !S::ENABLED
            && let Kind::Array(items) = &self.kind
        {
            for item in items.iter() {
                let r = item.eval::<S, C, I>(s);
                if !r.ok() {
                    return (r, false);
                }
                buf.push(r.value);
            }
            return (Res::of(Val::Ref(ValueRef::Null)), true);
        }
        (self.eval::<S, C, I>(s), false)
    }
}

fn call<'a, S: Slot<'a>, C: ?Sized, I: Input<C> + ?Sized>(
    target: &'a Call,
    args: &'a [Node],
    s: &Scope<'a, C, I>,
) -> Res<'a, S> {
    let name = match target {
        Call::StartsWith => {
            let mut vals = Buf::<2>::new();
            let trace = match eval_items::<S, C, I, 2>(args, s, &mut vals) {
                Ok(trace) => trace,
                Err(r) => return r,
            };
            return call_result(crate::stdlib::call_starts_with(&vals)).with_trace(trace);
        }
        Call::Named(name) => name,
    };
    if let Some(function) = s.env.functions.get(&**name) {
        if let Err(e) = function.check_arity(args.len()) {
            return Res::failed(e);
        }
        let mut vals = Buf::<4>::new();
        let trace = match eval_items::<S, C, I, 4>(args, s, &mut vals) {
            Ok(trace) => trace,
            Err(r) => return r,
        };
        return call_result(function.call(s.ctx, &vals)).with_trace(trace);
    }
    if let Some(macro_) = s.env.macros.get(&**name) {
        if !args.is_empty() {
            return Res::failed(Error::MacroArgs {
                name: name.to_string(),
                got: args.len(),
            });
        }
        let mut r = macro_.rule.root.eval::<S, C, I>(s);
        if S::ENABLED
            && let Some(root) = r.trace.take_frag()
        {
            // Go wraps the expansion in a node for the macro source; the call
            // node adopts that node's children, i.e. the expansion's root.
            let frag = combine([Some(root)]);
            r.trace.put_frag(frag);
        }
        return r;
    }
    Res::failed(Error::UnknownFunction(name.to_string()))
}

/// Go `evalItems`: evaluate array items or call arguments in order into
/// `vals`, stopping at the first incomplete item, which is returned as the
/// error. When tracing, the trace holds every item's trace, with the items
/// after a stop marked pruned.
// The error is a whole result by design: boxing it would allocate on the
// missing-field path, which must stay allocation-free.
#[allow(clippy::result_large_err)]
fn eval_items<'a, S: Slot<'a>, C: ?Sized, I: Input<C> + ?Sized, const N: usize>(
    items: &'a [Node],
    s: &Scope<'a, C, I>,
    vals: &mut Buf<'a, N>,
) -> Result<Option<Frag<'a>>, Res<'a, S>> {
    let mut traces: Vec<Option<Frag<'a>>> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let mut r = item.eval::<S, C, I>(s);
        if S::ENABLED {
            traces.push(r.trace.take_frag());
        }
        if !r.ok() {
            if S::ENABLED {
                traces.extend(items[i + 1..].iter().map(Node::pruned));
                let frag = combine(traces);
                r.trace.put_frag(frag);
            }
            return Err(r);
        }
        vals.push(r.value);
    }
    Ok(if S::ENABLED { combine(traces) } else { None })
}

/// Go `ArrayValue.Eval`: the items as an owned list.
fn eval_array<'a, S: Slot<'a>, C: ?Sized, I: Input<C> + ?Sized>(
    items: &'a [Node],
    s: &Scope<'a, C, I>,
) -> Res<'a, S> {
    let mut vals = Buf::<8>::new();
    match eval_items::<S, C, I, 8>(items, s, &mut vals) {
        Ok(trace) => {
            let values = vals.into_iter().map(Val::into_owned).collect();
            Res::of(Val::Owned(Value::Array(values))).with_trace(trace)
        }
        Err(r) => r,
    }
}

/// Go `nodeAnd`/`nodeOr` after short-circuiting: if exactly one side is
/// incomplete, return it; otherwise merge errors and missing fields, with the
/// right operand's value (as in JS) only when both sides completed.
fn merge<'a, S: Slot<'a>>(mut left: Res<'a, S>, mut right: Res<'a, S>) -> Res<'a, S> {
    match (left.ok(), right.ok()) {
        // Exactly one side is incomplete: return it, keeping both traces.
        (true, false) => {
            let trace = if S::ENABLED {
                combine([left.trace.take_frag(), right.trace.take_frag()])
            } else {
                None
            };
            right.with_trace(trace)
        }
        (false, true) => {
            let trace = if S::ENABLED {
                combine([left.trace.take_frag(), right.trace.take_frag()])
            } else {
                None
            };
            left.with_trace(trace)
        }
        (both_ok, _) => {
            let value = if both_ok {
                right.value
            } else {
                Val::Ref(ValueRef::Null)
            };
            let trace = if S::ENABLED {
                combine([left.trace.take_frag(), right.trace.take_frag()])
            } else {
                None
            };
            Res {
                value,
                problem: coalesce(left.problem, right.problem),
                missing: union(left.missing, right.missing),
                trace: S::empty(),
            }
            .with_trace(trace)
        }
    }
}

/// Go `nodeMatch.apply`: a regex matches a value's text, or any element of
/// a list with a text form.
fn matches(value: ValueRef<'_>, regex: ValueRef<'_>) -> bool {
    let ValueRef::Regex(re) = regex else {
        return false;
    };
    match value {
        ValueRef::Array(items) => items
            .iter()
            .any(|item| item.text().is_some_and(|t| re.is_match(&t))),
        _ => value.text().is_some_and(|t| re.is_match(&t)),
    }
}

/// A function call's result (Go: the function's `Result`).
fn call_result<'a, S: Slot<'a>>(result: Result<Val<'a>, CallFailure>) -> Res<'a, S> {
    match result {
        Ok(v) => Res::of(v),
        Err(CallFailure::Error(e)) => Res::failed(e),
        Err(CallFailure::Failed(source)) => Res::failed(Error::Function {
            name: String::new(),
            source,
        }),
        Err(CallFailure::Missing(fields)) => Res {
            problem: Some(Box::new(Problem {
                error: None,
                missing: fields,
            })),
            ..Res::of(Val::Ref(ValueRef::Null))
        },
    }
}
