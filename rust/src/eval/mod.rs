//! Compiled rules and evaluation (port of `ast.go` `lowerAST`, `nodes.go`,
//! `values.go`, and `functions.go`).

mod compare;

use smallvec::SmallVec;

use crate::ast::{Ast, LiteralKind, NodeData, NodeId, Operator, Segment};
use crate::env::Env;
use crate::error::{Error, ParseError};
use crate::input::Input;
use crate::literal::parse_literal;
use crate::print::path_string;
use crate::value::{ArrayRef, Val, Value, ValueRef};
use compare::{CmpOp, compare, compare_slice};

/// A compiled expression node.
#[derive(Clone, Debug)]
pub(crate) enum Node {
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
    /// An array whose items are all literals, built once.
    ConstArray(Box<[Value]>),
    Call {
        target: Call,
        args: Box<[Node]>,
    },
}

#[derive(Clone, Debug)]
pub(crate) enum Call {
    StartsWith,
    Named(Box<str>),
}

/// Lower an AST node (Go `lowerAST`). Literal values are parsed here; the
/// first invalid literal, in evaluation order, is the error.
pub(crate) fn lower(ast: &Ast, id: NodeId) -> Result<Node, ParseError> {
    Ok(match ast.data(id) {
        NodeData::Literal { span, kind } => Node::Literal(
            parse_literal(*kind, ast.text(*span))
                .map_err(|err| ParseError::at(ast.source(), span.start, err))?,
        ),
        NodeData::Path { segments, .. } => Node::Path {
            segments: segments.clone(),
            text: path_string(segments).into_boxed_str(),
        },
        NodeData::Array { items, .. } => {
            let items = items
                .iter()
                .map(|&item| lower(ast, item))
                .collect::<Result<Vec<_>, _>>()?;
            if items.iter().all(|item| matches!(item, Node::Literal(_))) {
                let values = items.into_iter().map(|item| match item {
                    Node::Literal(v) => v,
                    _ => unreachable!("checked above"),
                });
                Node::ConstArray(values.collect())
            } else {
                Node::Array(items.into_boxed_slice())
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
            Node::Call {
                target,
                args: args.into_boxed_slice(),
            }
        }
        NodeData::Unary { operand, .. } => Node::Not(Box::new(lower(ast, *operand)?)),
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
            let cmp = |op| Node::Compare {
                op,
                lhs: lhs.clone(),
                rhs: rhs.clone(),
            };
            let node = match op {
                Operator::And => Node::And(lhs, rhs),
                Operator::Or => Node::Or(lhs, rhs),
                Operator::Eq => cmp(CmpOp::Eq),
                Operator::Ne => cmp(CmpOp::Ne),
                Operator::Gt => cmp(CmpOp::Gt),
                Operator::Ge => cmp(CmpOp::Ge),
                Operator::Lt => cmp(CmpOp::Lt),
                Operator::Le => cmp(CmpOp::Le),
                Operator::Contains => cmp(CmpOp::Contains),
                Operator::Matches => Node::Match { lhs, rhs },
                // `x in <CIDR>` is CIDR containment, i.e. `x == <CIDR>`.
                Operator::In if rhs_is_cidr => cmp(CmpOp::Eq),
                Operator::In => Node::In { lhs, rhs },
                Operator::Not => unreachable!("not is unary"),
            };
            if *negated {
                Node::Not(Box::new(node))
            } else {
                node
            }
        }
    })
}

/// Missing field names, borrowed from compiled rules.
pub(crate) type Missing<'a> = SmallVec<[&'a str; 2]>;

/// The outcome of evaluating a rule.
#[derive(Debug)]
pub struct EvalResult<'a> {
    value: Val<'a>,
    error: Option<Box<Error>>,
    missing: Missing<'a>,
}

impl<'a> EvalResult<'a> {
    fn value(value: Val<'a>) -> Self {
        EvalResult {
            value,
            error: None,
            missing: Missing::new(),
        }
    }

    fn bool(b: bool) -> Self {
        Self::value(Val::Ref(ValueRef::Bool(b)))
    }

    fn error(error: Error) -> Self {
        EvalResult {
            value: Val::Ref(ValueRef::Null),
            error: Some(Box::new(error)),
            missing: Missing::new(),
        }
    }

    /// Keep only the error and missing fields (Go returns these without a value).
    fn incomplete(self) -> Self {
        EvalResult {
            value: Val::Ref(ValueRef::Null),
            error: self.error,
            missing: self.missing,
        }
    }

    /// The result value; `Null` when the rule did not produce one.
    pub fn value_ref(&self) -> ValueRef<'_> {
        self.value.as_ref()
    }

    pub fn into_value(self) -> Val<'a> {
        self.value
    }

    pub fn error_ref(&self) -> Option<&Error> {
        self.error.as_deref()
    }

    /// Fields the rule needed but the input lacked.
    pub fn missing_fields(&self) -> &[&'a str] {
        &self.missing
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
        if !out.contains(&name) {
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
    pub(crate) fn eval<'a, C: ?Sized, I: Input<C> + ?Sized>(
        &'a self,
        s: &Scope<'a, C, I>,
    ) -> EvalResult<'a> {
        match self {
            Node::And(l, r) => {
                let left = l.eval(s);
                if left.fail() {
                    return left;
                }
                let right = r.eval(s);
                if right.fail() {
                    return right;
                }
                merge(left, right, |l, r| l.pass() && r.pass())
            }
            Node::Or(l, r) => {
                let left = l.eval(s);
                if left.pass() {
                    return left;
                }
                let right = r.eval(s);
                if right.pass() {
                    return right;
                }
                merge(left, right, |l, r| l.pass() || r.pass())
            }
            Node::Not(inner) => {
                let r = inner.eval(s);
                if !r.ok() {
                    return r.incomplete();
                }
                EvalResult::bool(r.value.as_ref().is_zero())
            }
            Node::Compare { op, lhs, rhs } => {
                let (mut lbuf, mut rbuf) = (Buf::<8>::new(), Buf::<8>::new());
                let lv = match lhs.operand(s, &mut lbuf) {
                    Ok(v) => v,
                    Err(r) => return r,
                };
                let rv = match rhs.operand(s, &mut rbuf) {
                    Ok(v) => v,
                    Err(r) => return r,
                };
                EvalResult::bool(compare(view(&lv, &lbuf), *op, view(&rv, &rbuf)).pass)
            }
            Node::Match { lhs, rhs } => {
                let (mut lbuf, mut rbuf) = (Buf::<8>::new(), Buf::<8>::new());
                let lv = match lhs.operand(s, &mut lbuf) {
                    Ok(v) => v,
                    Err(r) => return r,
                };
                let rv = match rhs.operand(s, &mut rbuf) {
                    Ok(v) => v,
                    Err(r) => return r,
                };
                EvalResult::bool(matches(view(&lv, &lbuf), view(&rv, &rbuf)))
            }
            Node::In { lhs, rhs } => {
                let (mut lbuf, mut rbuf) = (Buf::<8>::new(), Buf::<8>::new());
                let lv = match lhs.operand(s, &mut lbuf) {
                    Ok(v) => v,
                    Err(r) => return r,
                };
                let rv = match rhs.operand(s, &mut rbuf) {
                    Ok(v) => v,
                    Err(r) => return r,
                };
                let (lv, rv) = (view(&lv, &lbuf), view(&rv, &rbuf));
                let ValueRef::Array(_) = rv else {
                    // The right side must be a list (the parser guarantees it).
                    return EvalResult::value(Val::Ref(ValueRef::Null));
                };
                // `x in list` is `list contains x`; a list-valued x is in the
                // list when ANY of its elements is.
                let outcome = match lv {
                    ValueRef::Array(items) => {
                        compare_slice(items, CmpOp::Eq, |el, _| compare(rv, CmpOp::Contains, el))
                    }
                    _ => compare(rv, CmpOp::Contains, lv),
                };
                EvalResult::bool(outcome.pass)
            }
            Node::Literal(v) => EvalResult::value(Val::Ref(v.as_ref())),
            Node::ConstArray(items) => {
                EvalResult::value(Val::Ref(ValueRef::Array(ArrayRef::Values(items))))
            }
            Node::Path { segments, text } => match s.input.get(s.ctx, segments) {
                Ok(Some(v)) => EvalResult::value(v),
                Ok(None) => EvalResult {
                    value: Val::Ref(ValueRef::Null),
                    error: None,
                    missing: smallvec_one(text),
                },
                Err(source) => EvalResult::error(Error::Input {
                    field: text.to_string(),
                    source,
                }),
            },
            Node::Array(items) => {
                let mut values = Vec::with_capacity(items.len());
                for item in items.iter() {
                    let r = item.eval(s);
                    if !r.ok() {
                        return r;
                    }
                    values.push(r.value.into_owned());
                }
                EvalResult::value(Val::Owned(Value::Array(values)))
            }
            Node::Call { target, args } => self.call(target, args, s),
        }
    }

    /// Evaluate an operand of a comparison. A non-constant array literal is
    /// evaluated into `buf` (returning `None`) so comparing against it does
    /// not allocate. A non-ok operand ends the comparison with its error and
    /// missing fields.
    fn operand<'a, C: ?Sized, I: Input<C> + ?Sized, const N: usize>(
        &'a self,
        s: &Scope<'a, C, I>,
        buf: &mut Buf<'a, N>,
    ) -> Result<Option<Val<'a>>, EvalResult<'a>> {
        if let Node::Array(items) = self {
            for item in items.iter() {
                let r = item.eval(s);
                if !r.ok() {
                    return Err(r.incomplete());
                }
                buf.push(r.value);
            }
            return Ok(None);
        }
        let r = self.eval(s);
        if !r.ok() {
            return Err(r.incomplete());
        }
        Ok(Some(r.value))
    }

    fn call<'a, C: ?Sized, I: Input<C> + ?Sized>(
        &'a self,
        target: &'a Call,
        args: &'a [Node],
        s: &Scope<'a, C, I>,
    ) -> EvalResult<'a> {
        let name = match target {
            Call::StartsWith => {
                let mut vals = Buf::<2>::new();
                if let Err(r) = eval_args(args, s, &mut vals) {
                    return r;
                }
                return match starts_with(&vals) {
                    Ok(b) => EvalResult::bool(b),
                    Err(e) => EvalResult::error(e),
                };
            }
            Call::Named(name) => name,
        };
        if let Some(function) = s.env.functions.get(&**name) {
            if function.args().len() != args.len() {
                return EvalResult::error(Error::ArgCount {
                    function: name.to_string(),
                    expected: function.args().len(),
                    got: args.len(),
                });
            }
            let mut vals = Buf::<4>::new();
            if let Err(r) = eval_args(args, s, &mut vals) {
                return r;
            }
            return match function.call(name, s.ctx, &vals) {
                Ok(v) => EvalResult::value(v),
                Err(e) => EvalResult::error(e),
            };
        }
        if let Some(macro_) = s.env.macros.get(&**name) {
            if !args.is_empty() {
                return EvalResult::error(Error::MacroArgs {
                    name: name.to_string(),
                    got: args.len(),
                });
            }
            return macro_.rule.root.eval(s);
        }
        EvalResult::error(Error::UnknownFunction(name.to_string()))
    }
}

fn smallvec_one(text: &str) -> Missing<'_> {
    let mut missing = Missing::new();
    missing.push(text);
    missing
}

/// Evaluate call arguments in order; the first non-ok argument is the result.
fn eval_args<'a, C: ?Sized, I: Input<C> + ?Sized, const N: usize>(
    args: &'a [Node],
    s: &Scope<'a, C, I>,
    vals: &mut Buf<'a, N>,
) -> Result<(), EvalResult<'a>> {
    for arg in args {
        let r = arg.eval(s);
        if !r.ok() {
            return Err(r);
        }
        vals.push(r.value);
    }
    Ok(())
}

/// Borrow an operand evaluated by [`Node::operand`].
fn view<'b, const N: usize>(value: &'b Option<Val<'_>>, buf: &'b Buf<'_, N>) -> ValueRef<'b> {
    match value {
        Some(v) => v.as_ref(),
        None => ValueRef::Array(ArrayRef::Vals(buf.as_slice())),
    }
}

/// Go `nodeAnd`/`nodeOr` after short-circuiting: if exactly one side is
/// incomplete, return it; otherwise merge errors and missing fields, with a
/// value only when both sides completed.
fn merge<'a>(
    left: EvalResult<'a>,
    right: EvalResult<'a>,
    value: impl Fn(&EvalResult, &EvalResult) -> bool,
) -> EvalResult<'a> {
    match (left.ok(), right.ok()) {
        (true, false) => right,
        (false, true) => left,
        (true, true) => EvalResult::bool(value(&left, &right)),
        (false, false) => EvalResult {
            value: Val::Ref(ValueRef::Null),
            error: coalesce(left.error, right.error),
            missing: union(left.missing, right.missing),
        },
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
