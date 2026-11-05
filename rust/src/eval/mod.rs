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
use crate::error::{Error, ParseError};
use crate::func::CallFailure;
use crate::input::Input;
use crate::literal::parse_literal;
use crate::opts::Opts;
use crate::print::{canonical_all, path_string};
use crate::value::{ArrayRef, Val, Value, ValueRef};
use compare::{CmpOp, compare, compare_in, compare_slice};
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
    /// `??`: the left side unless it is absent, else the right side.
    Coalesce(Box<Node>, Box<Node>),
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
    /// `compact(list)`: evaluates its array literal argument item by item.
    Compact,
    /// `coalesce(value, ...values)`: the first argument that is not absent.
    Coalesce,
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
            let target = match name {
                "starts_with" => Call::StartsWith,
                "compact" => Call::Compact,
                "coalesce" => Call::Coalesce,
                _ => Call::Named(name.into()),
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
                (Operator::Coalesce, _) => Kind::Coalesce(lhs, rhs),
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
/// let opts = Opts::default();
/// let result = rule.eval(&NoInput, &opts);
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

    /// The result value, owned or borrowed from the rule, input, or options.
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

    /// Missing, or a null value: what `??` and `coalesce` skip.
    /// An error is never absent.
    fn absent(&self) -> bool {
        self.error().is_none()
            && (!self.complete() || matches!(self.value.as_ref(), ValueRef::Null))
    }
}

/// Evaluate a rule's root node, with or without a trace.
pub(crate) fn run<'a, I: Input + ?Sized>(
    root: &'a Node,
    scope: &Scope<'a, I>,
    trace: bool,
) -> EvalResult<'a> {
    if trace {
        EvalResult(root.eval::<Option<Frag<'a>>, I>(scope))
    } else {
        root.eval::<(), I>(scope).untraced()
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
pub(crate) struct Scope<'a, I: ?Sized> {
    pub input: &'a I,
    pub opts: &'a Opts,
}

type Buf<'a, const N: usize> = SmallVec<[Val<'a>; N]>;

impl Node {
    /// Evaluate the node; when tracing, describe it (Go `tracedRule`).
    pub(crate) fn eval<'a, S: Slot<'a>, I: Input + ?Sized>(
        &'a self,
        s: &Scope<'a, I>,
    ) -> Res<'a, S> {
        let r = self.eval_kind::<S, I>(s);
        self.describe(r)
    }

    /// When tracing, wrap the trace `r` produced for this node in the node's
    /// description.
    fn describe<'a, S: Slot<'a>>(&'a self, mut r: Res<'a, S>) -> Res<'a, S> {
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

    fn eval_kind<'a, S: Slot<'a>, I: Input + ?Sized>(&'a self, s: &Scope<'a, I>) -> Res<'a, S> {
        match &self.kind {
            Kind::And(l, r) => {
                let mut left = l.eval::<S, I>(s);
                if left.fail() {
                    if S::ENABLED {
                        let frag = combine([left.trace.take_frag(), r.pruned()]);
                        left.trace.put_frag(frag);
                    }
                    return left;
                }
                let mut right = r.eval::<S, I>(s);
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
                let mut left = l.eval::<S, I>(s);
                if left.pass() {
                    if S::ENABLED {
                        let frag = combine([left.trace.take_frag(), r.pruned()]);
                        left.trace.put_frag(frag);
                    }
                    return left;
                }
                let mut right = r.eval::<S, I>(s);
                if right.pass() {
                    if S::ENABLED {
                        let frag = combine([left.trace.take_frag(), right.trace.take_frag()]);
                        right.trace.put_frag(frag);
                    }
                    return right;
                }
                merge::<S>(left, right)
            }
            Kind::Coalesce(l, r) => {
                let mut left = l.eval::<S, I>(s);
                if !left.absent() {
                    if S::ENABLED {
                        let frag = combine([left.trace.take_frag(), r.pruned()]);
                        left.trace.put_frag(frag);
                    }
                    return left;
                }
                let mut right = r.eval::<S, I>(s);
                if S::ENABLED {
                    let frag = combine([left.trace.take_frag(), right.trace.take_frag()]);
                    right.trace.put_frag(frag);
                }
                right
            }
            Kind::Not(inner) => {
                let mut r = inner.eval::<S, I>(s);
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
            Kind::Compare { op, lhs, rhs } => {
                // `!=` is decided by a failing comparison, the rest by a
                // passing one.
                self.binary::<S, I>(lhs, rhs, s, *op != CmpOp::Ne, |lv, rv| {
                    let outcome = compare(lv, *op, rv);
                    (
                        outcome.pass,
                        if S::ENABLED {
                            Diagnostic::new(outcome.diagnostic, lv, *op, rv)
                        } else {
                            None
                        },
                    )
                })
            }
            Kind::Match { lhs, rhs } => {
                self.binary::<S, I>(lhs, rhs, s, true, |lv, rv| (matches(lv, rv), None))
            }
            Kind::In { lhs, rhs } => self.binary::<S, I>(lhs, rhs, s, true, |lv, rv| {
                // A right side that is not a list, such as a field holding a
                // string, contains nothing.
                let ValueRef::Array(list) = rv else {
                    return (
                        false,
                        S::ENABLED.then(|| Diagnostic::in_invalid_shape(lv, rv)),
                    );
                };
                // `x in list` is `list contains x`, except a regex element
                // matches x like `x == /re/`; a list-valued x is in the list
                // when ANY of its elements is.
                let outcome = match lv {
                    ValueRef::Array(items) => {
                        compare_slice(items, CmpOp::Eq, |el, _| compare_in(el, list))
                    }
                    _ => compare_in(lv, list),
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
                    return eval_array::<S, I>(items, s);
                }
                Res::of(Val::Ref(ValueRef::Array(ArrayRef::Values(values))))
            }
            Kind::Path { segments, text } => match s.input.get(segments) {
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
            Kind::Array(items) => eval_array::<S, I>(items, s),
            Kind::Call { target, args } => call::<S, I>(target, args, s),
        }
    }

    /// Go `nodeCompare`/`nodeMatch`/`nodeIn`: evaluate both operands (a
    /// plain operand that is not ok prunes the right), then apply `f`, which
    /// returns the result and, when tracing, a diagnostic.
    ///
    /// An array literal operand with missing or failing items still lets the
    /// comparison run on its present items. The result is final when `f`
    /// returns `decisive` (a passing comparison, or a failing `!=`); otherwise
    /// the incomplete operands are merged like `and`/`or` do.
    fn binary<'a, S: Slot<'a>, I: Input + ?Sized>(
        &'a self,
        lhs: &'a Node,
        rhs: &'a Node,
        s: &Scope<'a, I>,
        decisive: bool,
        f: impl FnOnce(ValueRef<'_>, ValueRef<'_>) -> (bool, Option<Diagnostic>),
    ) -> Res<'a, S> {
        let (mut lbuf, mut rbuf) = (Buf::<8>::new(), Buf::<8>::new());
        let (mut left, left_in_buf) = lhs.operand::<S, I, 8>(s, &mut lbuf);
        if !left.ok() && !left_in_buf {
            let trace = if S::ENABLED {
                combine([left.trace.take_frag(), rhs.pruned()])
            } else {
                None
            };
            return left.incomplete(trace);
        }
        let (mut right, right_in_buf) = rhs.operand::<S, I, 8>(s, &mut rbuf);
        let incomplete = !left.ok() || !right.ok();
        if !right.ok() && !right_in_buf {
            return merge::<S>(left, right);
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
        if incomplete && pass != decisive {
            return merge::<S>(left, right);
        }
        let mut trace = if S::ENABLED {
            combine([left.trace.take_frag(), right.trace.take_frag()])
        } else {
            None
        };
        // A decided result of an incomplete operand carries no diagnostic.
        if let Some(diagnostic) = diagnostic.filter(|_| !incomplete) {
            trace = trace::add_diagnostic(trace, diagnostic);
        }
        Res::bool(pass).with_trace(trace)
    }

    /// Evaluate a comparison operand. An array literal is evaluated item by
    /// item into `buf` (the bool is true) so comparing against it does not
    /// allocate, and an incomplete item does not stop the others.
    fn operand<'a, S: Slot<'a>, I: Input + ?Sized, const N: usize>(
        &'a self,
        s: &Scope<'a, I>,
        buf: &mut Buf<'a, N>,
    ) -> (Res<'a, S>, bool) {
        if let Kind::Array(items) = &self.kind {
            return (self.array_literal::<S, I, N>(items, s, buf, false), true);
        }
        (self.eval::<S, I>(s), false)
    }

    /// Evaluate the items of this array literal into `buf` (see
    /// [`eval_elements`]) and describe the array node. The result has no
    /// value unless every item is present, and then only when tracing.
    fn array_literal<'a, S: Slot<'a>, I: Input + ?Sized, const N: usize>(
        &'a self,
        items: &'a [Node],
        s: &Scope<'a, I>,
        buf: &mut Buf<'a, N>,
        stop_on_error: bool,
    ) -> Res<'a, S> {
        let mut r = eval_elements::<S, I, N>(items, s, buf, stop_on_error);
        if S::ENABLED && r.complete() {
            let values = buf.iter().map(|v| v.as_ref().to_owned()).collect();
            r.value = Val::Owned(Value::Array(values));
        }
        self.describe(r)
    }
}

fn call<'a, S: Slot<'a>, I: Input + ?Sized>(
    target: &'a Call,
    args: &'a [Node],
    s: &Scope<'a, I>,
) -> Res<'a, S> {
    let name = match target {
        Call::StartsWith => {
            let mut vals = Buf::<2>::new();
            let trace = match eval_items::<S, I, 2>(args, s, &mut vals) {
                Ok(trace) => trace,
                Err(r) => return r,
            };
            return call_result(crate::stdlib::call_starts_with(&vals)).with_trace(trace);
        }
        Call::Compact => return compact::<S, I>(args, s),
        Call::Coalesce => return coalesce_call::<S, I>(args, s),
        Call::Named(name) => name,
    };
    if let Some(function) = s.opts.function(name) {
        if let Err(e) = function.check_arity(args.len()) {
            return Res::failed(e);
        }
        let mut vals = Buf::<4>::new();
        let trace = match eval_items::<S, I, 4>(args, s, &mut vals) {
            Ok(trace) => trace,
            Err(r) => return r,
        };
        return call_result(function.call(&vals)).with_trace(trace);
    }
    if let Some(macro_) = s.opts.macro_rule(name) {
        if !args.is_empty() {
            return Res::failed(Error::MacroArgs {
                name: name.to_string(),
                got: args.len(),
            });
        }
        let mut r = macro_.rule.root.eval::<S, I>(s);
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
fn eval_items<'a, S: Slot<'a>, I: Input + ?Sized, const N: usize>(
    items: &'a [Node],
    s: &Scope<'a, I>,
    vals: &mut Buf<'a, N>,
) -> Result<Option<Frag<'a>>, Res<'a, S>> {
    let mut traces: Vec<Option<Frag<'a>>> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let mut r = item.eval::<S, I>(s);
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

/// Evaluate array literal items in order for a comparison or `compact`,
/// without stopping at an incomplete item: the present values go to
/// `present`, and the returned result (with no value) is incomplete when an
/// item was, holding the union of the items' missing fields and the first
/// error. With `stop_on_error`, an item that failed ends the evaluation and
/// the items after it are pruned in the trace.
fn eval_elements<'a, S: Slot<'a>, I: Input + ?Sized, const N: usize>(
    items: &'a [Node],
    s: &Scope<'a, I>,
    present: &mut Buf<'a, N>,
    stop_on_error: bool,
) -> Res<'a, S> {
    let mut out = Res::<S>::of(Val::Ref(ValueRef::Null));
    let mut traces: Vec<Option<Frag<'a>>> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let mut r = item.eval::<S, I>(s);
        if S::ENABLED {
            traces.push(r.trace.take_frag());
        }
        if r.ok() {
            present.push(r.value);
            continue;
        }
        if stop_on_error && r.error().is_some() {
            // The failing item is the whole result, with its own missing
            // fields.
            out.missing = r.missing;
            out.problem = r.problem;
            if S::ENABLED {
                traces.extend(items[i + 1..].iter().map(Node::pruned));
            }
            break;
        }
        out.missing = union(std::mem::take(&mut out.missing), r.missing);
        out.problem = keep_first_error(out.problem, r.problem);
    }
    if S::ENABLED {
        out.trace.put_frag(combine(traces));
    }
    out
}

/// Like [`coalesce`], but with only the first error kept.
fn keep_first_error(
    first: Option<Box<Problem>>,
    next: Option<Box<Problem>>,
) -> Option<Box<Problem>> {
    match (first, next) {
        (Some(mut first), Some(next)) => {
            let next = *next;
            first.error = first.error.or(next.error);
            for name in next.missing {
                if !first.missing.contains(&name) {
                    first.missing.push(name);
                }
            }
            Some(first)
        }
        (first, next) => first.or(next),
    }
}

/// `coalesce(value, ...values)`: the same as `value ?? ... ?? last`.
fn coalesce_call<'a, S: Slot<'a>, I: Input + ?Sized>(
    args: &'a [Node],
    s: &Scope<'a, I>,
) -> Res<'a, S> {
    let Some((last, init)) = args.split_last() else {
        unreachable!("coalesce has at least one argument, checked at parse time");
    };
    let mut traces: Vec<Option<Frag<'a>>> = Vec::new();
    for (i, arg) in init.iter().enumerate() {
        let mut r = arg.eval::<S, I>(s);
        if S::ENABLED {
            traces.push(r.trace.take_frag());
        }
        if !r.absent() {
            if S::ENABLED {
                traces.extend(args[i + 1..].iter().map(Node::pruned));
                r.trace.put_frag(combine(traces));
            }
            return r;
        }
    }
    // Every other argument is absent: the last one is the result as it is.
    let mut r = last.eval::<S, I>(s);
    if S::ENABLED {
        traces.push(r.trace.take_frag());
        r.trace.put_frag(combine(traces));
    }
    r
}

/// `compact(list)`: the list without its absent items. An array literal is
/// evaluated item by item, so missing items are dropped instead of making the
/// list incomplete; an item that fails is the result.
fn compact<'a, S: Slot<'a>, I: Input + ?Sized>(args: &'a [Node], s: &Scope<'a, I>) -> Res<'a, S> {
    let [arg] = args else {
        unreachable!("compact has one argument, checked at parse time");
    };
    if let Kind::Array(items) = &arg.kind {
        let mut present = Buf::<8>::new();
        let mut array = arg.array_literal::<S, I, 8>(items, s, &mut present, true);
        let trace = if S::ENABLED {
            combine([array.trace.take_frag()])
        } else {
            None
        };
        // An item error is the result, with its own missing fields.
        if array.error().is_some() {
            return array.with_trace(trace);
        }
        let values = present
            .into_iter()
            .map(Val::into_owned)
            .filter(|value| !matches!(value, Value::Null))
            .collect();
        return Res::of(Val::Owned(Value::Array(values))).with_trace(trace);
    }
    let mut vals = Buf::<1>::new();
    let trace = match eval_items::<S, I, 1>(args, s, &mut vals) {
        Ok(trace) => trace,
        Err(r) => return r,
    };
    match vals[0].as_ref() {
        ValueRef::Array(list) => {
            let values = list
                .iter()
                .filter(|item| !matches!(item, ValueRef::Null))
                .map(ValueRef::to_owned)
                .collect();
            Res::of(Val::Owned(Value::Array(values)))
        }
        other => Res::failed(Error::InvalidArg {
            name: "list".to_owned(),
            expected: "array".to_owned(),
            got: other.type_name().to_owned(),
        }),
    }
    .with_trace(trace)
}

/// Go `ArrayValue.Eval`: the items as an owned list.
fn eval_array<'a, S: Slot<'a>, I: Input + ?Sized>(
    items: &'a [Node],
    s: &Scope<'a, I>,
) -> Res<'a, S> {
    let mut vals = Buf::<8>::new();
    match eval_items::<S, I, 8>(items, s, &mut vals) {
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
