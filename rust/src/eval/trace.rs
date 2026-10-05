//! Evaluation traces.

use super::compare::{CmpOp, Diagnostic as Outcome};
use super::{EvalResult, Meta};
use crate::ast::{AstKind, NodeId};
use crate::value::{Value, ValueRef};

/// How a traced node ended.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TraceStatus {
    /// No status: internal grouping nodes.
    #[default]
    Unknown,
    /// Completed with a non-zero value.
    Passed,
    /// Completed with a zero value.
    Failed,
    /// Needed fields the input lacked.
    Missing,
    /// Failed with an error.
    Error,
    /// Not evaluated, because of short-circuiting.
    Pruned,
}

impl TraceStatus {
    /// `unknown`, `passed`, `failed`, `missing`, `error`, or `pruned`.
    pub fn name(self) -> &'static str {
        match self {
            TraceStatus::Unknown => "unknown",
            TraceStatus::Passed => "passed",
            TraceStatus::Failed => "failed",
            TraceStatus::Missing => "missing",
            TraceStatus::Error => "error",
            TraceStatus::Pruned => "pruned",
        }
    }
}

/// The kind of comparison problem a [`Diagnostic`] reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DiagnosticCode {
    /// The operand types cannot be compared (for example `bool == int64`).
    ComparisonIncomparable,
    /// The operands have the wrong shape for the operator (for example a
    /// list on the right of `contains`).
    ComparisonInvalidShape,
    /// The types are comparable, but not with this operator (for example
    /// `bool > bool`).
    ComparisonUnsupportedOperator,
}

impl DiagnosticCode {
    /// `comparison_incomparable`, `comparison_invalid_shape`, or
    /// `comparison_unsupported_operator`.
    pub fn name(self) -> &'static str {
        match self {
            DiagnosticCode::ComparisonIncomparable => "comparison_incomparable",
            DiagnosticCode::ComparisonInvalidShape => "comparison_invalid_shape",
            DiagnosticCode::ComparisonUnsupportedOperator => "comparison_unsupported_operator",
        }
    }
}

/// Why a comparison evaluated to false without comparing its operands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// The kind of problem.
    pub code: DiagnosticCode,
    /// A readable description, such as `cannot compare bool == int64`.
    pub message: String,
    /// Left operand type name, as in typed JSON (`int64`, `string`,
    /// `array`, ...).
    pub left_type: &'static str,
    /// Operator machine name (`eq`, `contains`, ...).
    pub operator: &'static str,
    /// Right operand type name.
    pub right_type: &'static str,
}

impl Diagnostic {
    pub(crate) fn new(
        outcome: Outcome,
        left: ValueRef<'_>,
        op: CmpOp,
        right: ValueRef<'_>,
    ) -> Option<Self> {
        let (lt, rt, sym) = (left.type_name(), right.type_name(), op.symbol());
        let (code, message) = match outcome {
            Outcome::None => return None,
            Outcome::Incomparable => (
                DiagnosticCode::ComparisonIncomparable,
                format!("cannot compare {lt} {sym} {rt}"),
            ),
            Outcome::InvalidShape => (
                DiagnosticCode::ComparisonInvalidShape,
                format!("invalid comparison shape for {lt} {sym} {rt}"),
            ),
            Outcome::UnsupportedOperator => (
                DiagnosticCode::ComparisonUnsupportedOperator,
                format!("operator {sym} is not supported for {lt} and {rt}"),
            ),
        };
        Some(Diagnostic {
            code,
            message,
            left_type: lt,
            operator: op.name(),
            right_type: rt,
        })
    }
}

/// How evaluation reached its result, node by node.
///
/// Returned by [`EvalResult::trace`] when [`Opts::trace`](crate::Opts::trace)
/// is set. The root describes the whole rule; `children` follow the
/// expression tree. A macro call has the macro's expression as its child.
///
/// ```rust
/// use rulekit::value::{Map, Value};
/// use rulekit::{KvInput, Opts, TraceStatus};
///
/// let rule = rulekit::parse("port == 443 or tls")?;
/// let input = KvInput::from_values(Map::from_iter([("port".to_owned(), Value::Int(443))]));
///
/// let result = rule.eval(&input, &(), Opts::default().with_trace(true));
/// let trace = result.trace().expect("tracing was on");
/// assert_eq!(trace.status, TraceStatus::Passed);
/// assert_eq!(trace.expr, "port == 443 or tls");
///
/// // `or` short-circuited: `tls` was never read.
/// let [left, right] = &trace.children[..] else { panic!() };
/// assert_eq!(left.status, TraceStatus::Passed);
/// assert_eq!(right.status, TraceStatus::Pruned);
/// # Ok::<(), rulekit::ParseError>(())
/// ```
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Trace {
    /// The AST node (of the rule, or of a macro's rule for nodes inside a
    /// macro expansion). `None` only for internal grouping nodes.
    pub node: Option<NodeId>,
    /// The node's kind; `None` only for internal grouping nodes.
    pub kind: Option<AstKind>,
    /// The node's canonical expression.
    pub expr: String,
    /// The node's value (`Null` if it produced none).
    pub value: Value,
    /// The error message, if the node failed.
    pub error: Option<String>,
    /// Fields the node needed but the input lacked.
    pub missing_fields: Vec<String>,
    /// Comparisons that evaluated to false because they could not be made.
    pub diagnostics: Vec<Diagnostic>,
    /// How the node ended.
    pub status: TraceStatus,
    /// Whether the node was evaluated.
    pub active: bool,
    /// Whether the node was skipped by short-circuiting.
    pub pruned: bool,
    /// Traces of the node's operands, arguments, or items.
    pub children: Vec<Trace>,
}

/// Go `traceStatus`.
pub(crate) fn status(r: &EvalResult<'_>) -> TraceStatus {
    if r.error.is_some() {
        TraceStatus::Error
    } else if !r.missing.is_empty() {
        TraceStatus::Missing
    } else if r.pass() {
        TraceStatus::Passed
    } else {
        TraceStatus::Failed
    }
}

/// Go `tracedRule.Eval`: describe a node, adopting the children and
/// diagnostics of the trace its evaluation produced.
pub(crate) fn wrap(meta: &Meta, r: &EvalResult<'_>, inner: Option<Box<Trace>>) -> Trace {
    let (children, diagnostics) = inner
        .map(|t| (t.children, t.diagnostics))
        .unwrap_or_default();
    Trace {
        node: Some(meta.id),
        kind: Some(meta.kind),
        expr: meta.expr.to_string(),
        value: r.value.as_ref().to_owned(),
        error: r.error.as_ref().map(ToString::to_string),
        missing_fields: r.missing.iter().map(|s| s.to_string()).collect(),
        diagnostics,
        status: status(r),
        active: true,
        pruned: false,
        children,
    }
}

/// Go `prunedTrace`.
pub(crate) fn pruned(meta: &Meta) -> Trace {
    Trace {
        node: Some(meta.id),
        kind: Some(meta.kind),
        expr: meta.expr.to_string(),
        status: TraceStatus::Pruned,
        pruned: true,
        ..Trace::default()
    }
}

/// Go `combineTrace`: an internal grouping node over the present children.
pub(crate) fn combine(
    children: impl IntoIterator<Item = Option<Box<Trace>>>,
) -> Option<Box<Trace>> {
    let children: Vec<Trace> = children.into_iter().flatten().map(|t| *t).collect();
    if children.is_empty() {
        return None;
    }
    Some(Box::new(Trace {
        children,
        ..Trace::default()
    }))
}
