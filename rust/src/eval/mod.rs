//! Compiled rules and evaluation (port of `ast.go` `lowerAST`, `nodes.go`,
//! `values.go`, `functions.go`, and `trace.go`).
//!
//! Evaluation is generic over `const TRACE: bool`. `Rule::eval` branches on
//! the trace flag once; the untraced instantiation contains no trace code.

mod compare;
#[doc(hidden)]
pub use compare::cmp_number;
pub(crate) mod trace;

use std::borrow::Cow;

use smallvec::SmallVec;

use crate::ast::{Ast, AstKind, LiteralKind, NodeData, NodeId, Operator, Segment};
use crate::env::Env;
use crate::error::{Error, ParseError};
use crate::input::Input;
use crate::literal::parse_literal;
use crate::print::{canonical, path_string};
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
                .map(|&item| lower(ast, item))
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
                .map(|&arg| lower(ast, arg))
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
        NodeData::Unary { operand, .. } => Kind::Not(Box::new(lower(ast, *operand)?)),
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
            let lhs = Box::new(lower(ast, *lhs)?);
            let rhs = Box::new(lower(ast, *rhs)?);
            let cmp = |op| Kind::Compare {
                op,
                lhs: lhs.clone(),
                rhs: rhs.clone(),
            };
            let base = match op {
                Operator::And => Kind::And(lhs, rhs),
                Operator::Or => Kind::Or(lhs, rhs),
                Operator::Eq => cmp(CmpOp::Eq),
                Operator::Ne => cmp(CmpOp::Ne),
                Operator::Gt => cmp(CmpOp::Gt),
                Operator::Ge => cmp(CmpOp::Ge),
                Operator::Lt => cmp(CmpOp::Lt),
                Operator::Le => cmp(CmpOp::Le),
                Operator::Contains => cmp(CmpOp::Contains),
                Operator::Matches => Kind::Match { lhs, rhs },
                // `x in <CIDR>` is CIDR containment, i.e. `x == <CIDR>`.
                Operator::In if rhs_is_cidr => cmp(CmpOp::Eq),
                Operator::In => Kind::In { lhs, rhs },
                Operator::Not => unreachable!("not is unary"),
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
        expr: canonical(ast, id).into_boxed_str(),
    };
    Ok(Node {
        kind,
        meta: Some(meta),
    })
}

/// Missing field names, borrowed from compiled rules.
pub(crate) type Missing<'a> = SmallVec<[Cow<'a, str>; 2]>;

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
/// let result = rule.eval(&NoInput, &(), Opts::default());
/// assert!(result.unknown());
/// assert_eq!(result.missing_fields(), ["port", "tls"]);
/// # Ok::<(), rulekit::ParseError>(())
/// ```
#[derive(Debug)]
pub struct EvalResult<'a> {
    value: Val<'a>,
    error: Option<Box<Error>>,
    missing: Missing<'a>,
    trace: Option<Frag<'a>>,
}

impl<'a> EvalResult<'a> {
    fn of(value: Val<'a>) -> Self {
        EvalResult {
            value,
            error: None,
            missing: Missing::new(),
            trace: None,
        }
    }

    fn bool(b: bool) -> Self {
        Self::of(Val::Ref(ValueRef::Bool(b)))
    }

    fn failed(error: Error) -> Self {
        EvalResult {
            error: Some(Box::new(error)),
            ..Self::of(Val::Ref(ValueRef::Null))
        }
    }

    /// Keep only the error and missing fields (Go returns these without a
    /// value), with the given trace.
    fn incomplete(self, trace: Option<Frag<'a>>) -> Self {
        EvalResult {
            value: Val::Ref(ValueRef::Null),
            error: self.error,
            missing: self.missing,
            trace,
        }
    }

    fn with_trace(mut self, trace: Option<Frag<'a>>) -> Self {
        self.trace = trace;
        self
    }

    /// The result value; `Null` when the rule did not produce one.
    pub fn value(&self) -> ValueRef<'_> {
        self.value.as_ref()
    }

    /// The result value, owned or borrowed from the rule, input, or context.
    pub fn into_value(self) -> Val<'a> {
        self.value
    }

    /// The evaluation error, if any. When both sides of `and`/`or` fail,
    /// this is an [`Error::Multiple`].
    pub fn error(&self) -> Option<&Error> {
        self.error.as_deref()
    }

    /// Fields the rule needed but the input lacked.
    pub fn missing_fields(&self) -> &[Cow<'a, str>] {
        &self.missing
    }

    /// The evaluation trace, when tracing was enabled.
    pub fn trace(&self) -> Option<&Trace<'a>> {
        self.trace.as_ref().and_then(Frag::node)
    }

    /// No error and no missing fields.
    pub fn complete(&self) -> bool {
        self.error.is_none() && self.missing.is_empty()
    }

    /// Same as [`complete`](Self::complete).
    pub fn ok(&self) -> bool {
        self.complete()
    }

    /// No error, but more input is needed.
    pub fn unknown(&self) -> bool {
        self.error.is_none() && !self.missing.is_empty()
    }

    /// Complete with a non-zero value.
    pub fn pass(&self) -> bool {
        self.complete() && !self.value.as_ref().is_zero()
    }

    /// Complete with a zero value.
    pub fn fail(&self) -> bool {
        self.complete() && self.value.as_ref().is_zero()
    }
}

/// Go `unionUnique`.
fn union<'a>(left: Missing<'a>, right: Missing<'a>) -> Missing<'a> {
    if left.is_empty() {
        return right;
    }
    let mut out = left;
    for name in right {
        if !out.iter().any(|existing| *existing == name) {
            out.push(name);
        }
    }
    out
}

/// Go `coalesceErrs`.
fn coalesce(left: Option<Box<Error>>, right: Option<Box<Error>>) -> Option<Box<Error>> {
    match (left, right) {
        (Some(l), Some(r)) => Some(Box::new(Error::Multiple(vec![*l, *r]))),
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
    /// Evaluate the node; with `TRACE`, describe it (Go `tracedRule`).
    pub(crate) fn eval<'a, const TRACE: bool, C: ?Sized, I: Input<C> + ?Sized>(
        &'a self,
        s: &Scope<'a, C, I>,
    ) -> EvalResult<'a> {
        let mut r = self.eval_kind::<TRACE, C, I>(s);
        if TRACE && let Some(meta) = &self.meta {
            let inner = r.trace.take();
            r.trace = Some(trace::wrap(meta, &r, inner));
        }
        r
    }

    /// Go `prunedTrace`.
    fn pruned(&self) -> Option<Frag<'_>> {
        self.meta.as_ref().map(trace::pruned)
    }

    fn eval_kind<'a, const TRACE: bool, C: ?Sized, I: Input<C> + ?Sized>(
        &'a self,
        s: &Scope<'a, C, I>,
    ) -> EvalResult<'a> {
        match &self.kind {
            Kind::And(l, r) => {
                let mut left = l.eval::<TRACE, C, I>(s);
                if left.fail() {
                    if TRACE {
                        left.trace = combine([left.trace.take(), r.pruned()]);
                    }
                    return left;
                }
                let mut right = r.eval::<TRACE, C, I>(s);
                if right.fail() {
                    if TRACE {
                        right.trace = combine([left.trace.take(), right.trace.take()]);
                    }
                    return right;
                }
                merge::<TRACE>(left, right, |l, r| l.pass() && r.pass())
            }
            Kind::Or(l, r) => {
                let mut left = l.eval::<TRACE, C, I>(s);
                if left.pass() {
                    if TRACE {
                        left.trace = combine([left.trace.take(), r.pruned()]);
                    }
                    return left;
                }
                let mut right = r.eval::<TRACE, C, I>(s);
                if right.pass() {
                    if TRACE {
                        right.trace = combine([left.trace.take(), right.trace.take()]);
                    }
                    return right;
                }
                merge::<TRACE>(left, right, |l, r| l.pass() || r.pass())
            }
            Kind::Not(inner) => {
                let mut r = inner.eval::<TRACE, C, I>(s);
                // A negated operator has no traced node between it and its
                // operands, so its operand traces become its children.
                let trace = if TRACE {
                    match r.trace.take() {
                        Some(t) if t.is_group() => Some(t),
                        other => combine([other]),
                    }
                } else {
                    None
                };
                if !r.ok() {
                    return r.incomplete(trace);
                }
                EvalResult::bool(r.value.as_ref().is_zero()).with_trace(trace)
            }
            Kind::Compare { op, lhs, rhs } => self.binary::<TRACE, C, I>(lhs, rhs, s, |lv, rv| {
                let outcome = compare(lv, *op, rv);
                (
                    outcome.pass,
                    if TRACE {
                        Diagnostic::new(outcome.diagnostic, lv, *op, rv)
                    } else {
                        None
                    },
                )
            }),
            Kind::Match { lhs, rhs } => {
                self.binary::<TRACE, C, I>(lhs, rhs, s, |lv, rv| (matches(lv, rv), None))
            }
            Kind::In { lhs, rhs } => self.binary::<TRACE, C, I>(lhs, rhs, s, |lv, rv| {
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
                let diagnostic = if TRACE {
                    Diagnostic::new(outcome.diagnostic, rv, CmpOp::Contains, lv)
                } else {
                    None
                };
                (outcome.pass, diagnostic)
            }),
            Kind::Literal(v) => EvalResult::of(Val::Ref(v.as_ref())),
            Kind::ConstArray(values, items) => {
                if TRACE {
                    return eval_array::<TRACE, C, I>(items, s);
                }
                EvalResult::of(Val::Ref(ValueRef::Array(ArrayRef::Values(values))))
            }
            Kind::Path { segments, text } => match s.input.get(s.ctx, segments) {
                Ok(Some(v)) => EvalResult::of(v),
                Ok(None) => {
                    let mut missing = Missing::new();
                    missing.push(Cow::Borrowed(&**text));
                    EvalResult {
                        missing,
                        ..EvalResult::of(Val::Ref(ValueRef::Null))
                    }
                }
                Err(source) => EvalResult::failed(Error::Input {
                    field: text.to_string(),
                    source,
                }),
            },
            Kind::Array(items) => eval_array::<TRACE, C, I>(items, s),
            Kind::Call { target, args } => call::<TRACE, C, I>(target, args, s),
        }
    }

    /// Go `nodeCompare`/`nodeMatch`/`nodeIn`: evaluate both operands (a
    /// non-ok left operand prunes the right), then apply `f`, which returns
    /// the result and, when tracing, a diagnostic.
    fn binary<'a, const TRACE: bool, C: ?Sized, I: Input<C> + ?Sized>(
        &'a self,
        lhs: &'a Node,
        rhs: &'a Node,
        s: &Scope<'a, C, I>,
        f: impl FnOnce(ValueRef<'_>, ValueRef<'_>) -> (bool, Option<Diagnostic>),
    ) -> EvalResult<'a> {
        let (mut lbuf, mut rbuf) = (Buf::<8>::new(), Buf::<8>::new());
        let (mut left, left_in_buf) = lhs.operand::<TRACE, C, I, 8>(s, &mut lbuf);
        if !left.ok() {
            let trace = if TRACE {
                combine([left.trace.take(), rhs.pruned()])
            } else {
                None
            };
            return left.incomplete(trace);
        }
        let (mut right, right_in_buf) = rhs.operand::<TRACE, C, I, 8>(s, &mut rbuf);
        let trace = if TRACE {
            combine([left.trace.take(), right.trace.take()])
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
        EvalResult::bool(pass).with_trace(trace)
    }

    /// Evaluate a comparison operand. Untraced, a non-constant array literal
    /// is evaluated into `buf` (the bool is true) so comparing against it does
    /// not allocate; traced, it is evaluated as a node like in Go.
    fn operand<'a, const TRACE: bool, C: ?Sized, I: Input<C> + ?Sized, const N: usize>(
        &'a self,
        s: &Scope<'a, C, I>,
        buf: &mut Buf<'a, N>,
    ) -> (EvalResult<'a>, bool) {
        if !TRACE && let Kind::Array(items) = &self.kind {
            for item in items.iter() {
                let r = item.eval::<TRACE, C, I>(s);
                if !r.ok() {
                    return (r, false);
                }
                buf.push(r.value);
            }
            return (EvalResult::of(Val::Ref(ValueRef::Null)), true);
        }
        (self.eval::<TRACE, C, I>(s), false)
    }
}

fn call<'a, const TRACE: bool, C: ?Sized, I: Input<C> + ?Sized>(
    target: &'a Call,
    args: &'a [Node],
    s: &Scope<'a, C, I>,
) -> EvalResult<'a> {
    let name = match target {
        Call::StartsWith => {
            let mut vals = Buf::<2>::new();
            let trace = match eval_items::<TRACE, C, I, 2>(args, s, &mut vals) {
                Ok(trace) => trace,
                Err(r) => return r,
            };
            let r = match starts_with(&vals) {
                Ok(b) => EvalResult::bool(b),
                Err(e) => EvalResult::failed(e),
            };
            return r.with_trace(trace);
        }
        Call::Named(name) => name,
    };
    if let Some(function) = s.env.functions.get(&**name) {
        if function.args().len() != args.len() {
            return EvalResult::failed(Error::ArgCount {
                function: name.to_string(),
                expected: function.args().len(),
                got: args.len(),
            });
        }
        let mut vals = Buf::<4>::new();
        let trace = match eval_items::<TRACE, C, I, 4>(args, s, &mut vals) {
            Ok(trace) => trace,
            Err(r) => return r,
        };
        let r = match function.call(name, s.ctx, &vals) {
            Ok(v) => EvalResult::of(v),
            Err(e) => EvalResult::failed(e),
        };
        return r.with_trace(trace);
    }
    if let Some(macro_) = s.env.macros.get(&**name) {
        if !args.is_empty() {
            return EvalResult::failed(Error::MacroArgs {
                name: name.to_string(),
                got: args.len(),
            });
        }
        let mut r = macro_.rule.root.eval::<TRACE, C, I>(s);
        if TRACE && let Some(root) = r.trace.take() {
            // Go wraps the expansion in a node for the macro source; the call
            // node adopts that node's children, i.e. the expansion's root.
            r.trace = combine([Some(root)]);
        }
        return r;
    }
    EvalResult::failed(Error::UnknownFunction(name.to_string()))
}

/// Go `evalItems`: evaluate array items or call arguments in order into
/// `vals`, stopping at the first incomplete item, which is returned as the
/// error. When tracing, the trace holds every item's trace, with the items
/// after a stop marked pruned.
fn eval_items<'a, const TRACE: bool, C: ?Sized, I: Input<C> + ?Sized, const N: usize>(
    items: &'a [Node],
    s: &Scope<'a, C, I>,
    vals: &mut Buf<'a, N>,
) -> Result<Option<Frag<'a>>, EvalResult<'a>> {
    let mut traces: Vec<Option<Frag<'a>>> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let mut r = item.eval::<TRACE, C, I>(s);
        if TRACE {
            traces.push(r.trace.take());
        }
        if !r.ok() {
            if TRACE {
                traces.extend(items[i + 1..].iter().map(Node::pruned));
                r.trace = combine(traces);
            }
            return Err(r);
        }
        vals.push(r.value);
    }
    Ok(if TRACE { combine(traces) } else { None })
}

/// Go `ArrayValue.Eval`: the items as an owned list.
fn eval_array<'a, const TRACE: bool, C: ?Sized, I: Input<C> + ?Sized>(
    items: &'a [Node],
    s: &Scope<'a, C, I>,
) -> EvalResult<'a> {
    let mut vals = Buf::<8>::new();
    match eval_items::<TRACE, C, I, 8>(items, s, &mut vals) {
        Ok(trace) => {
            let values = vals.into_iter().map(Val::into_owned).collect();
            EvalResult::of(Val::Owned(Value::Array(values))).with_trace(trace)
        }
        Err(r) => r,
    }
}

/// Go `nodeAnd`/`nodeOr` after short-circuiting: if exactly one side is
/// incomplete, return it; otherwise merge errors and missing fields,
/// with a value only when both sides completed.
fn merge<'a, const TRACE: bool>(
    mut left: EvalResult<'a>,
    mut right: EvalResult<'a>,
    value: impl Fn(&EvalResult, &EvalResult) -> bool,
) -> EvalResult<'a> {
    match (left.ok(), right.ok()) {
        // Exactly one side is incomplete: return it, keeping both traces.
        (true, false) => {
            let trace = if TRACE {
                combine([left.trace, right.trace.take()])
            } else {
                None
            };
            right.with_trace(trace)
        }
        (false, true) => {
            let trace = if TRACE {
                combine([left.trace.take(), right.trace])
            } else {
                None
            };
            left.with_trace(trace)
        }
        (both_ok, _) => {
            let value = if both_ok {
                Val::Ref(ValueRef::Bool(value(&left, &right)))
            } else {
                Val::Ref(ValueRef::Null)
            };
            EvalResult {
                value,
                error: coalesce(left.error, right.error),
                missing: union(left.missing, right.missing),
                trace: if TRACE {
                    combine([left.trace, right.trace])
                } else {
                    None
                },
            }
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

/// The `starts_with(value, prefix)` standard library function: both
/// arguments must be strings or have a text form.
fn starts_with(vals: &[Val<'_>]) -> Result<bool, Error> {
    let text = |i: usize, name: &str| {
        let value = vals[i].as_ref();
        value.text().ok_or_else(|| Error::InvalidArg {
            name: name.to_owned(),
            expected: "string".to_owned(),
            got: value.type_name().to_owned(),
        })
    };
    let value = text(0, "value")?;
    let prefix = text(1, "prefix")?;
    Ok(value.starts_with(&*prefix))
}
