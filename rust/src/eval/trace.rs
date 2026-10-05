//! Evaluation traces.

use super::compare::{CmpOp, Diagnostic as Outcome};
use std::borrow::Cow;

use super::{EvalResult, Meta, Missing};
use crate::ast::{AstKind, NodeId};
use crate::value::{Val, ValueRef};

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
/// is set. The root describes the whole rule; [`children`](Self::children)
/// follow the expression tree. A macro call has the macro's expression as its
/// child. A trace borrows from the rule and the input like the result does;
/// [`into_owned`](Self::into_owned) detaches it.
///
/// ```rust
/// use rulekit::value::{Map, Value};
/// use rulekit::{KvInput, Opts, TraceStatus};
///
/// let rule = rulekit::parse("port == 443 or tls")?;
/// let input = KvInput::from_values(Map::from_iter([("port".to_owned(), Value::Int(443))]));
///
/// let result = rule.eval(&(), &input, Opts::default().with_trace(true));
/// let trace = result.trace().expect("tracing was on");
/// assert_eq!(trace.status(), TraceStatus::Passed);
/// assert_eq!(trace.expr(), "port == 443 or tls");
///
/// // `or` short-circuited: `tls` was never read.
/// let [left, right] = trace.children() else { panic!() };
/// assert_eq!(left.status(), TraceStatus::Passed);
/// assert_eq!(right.status(), TraceStatus::Pruned);
/// # Ok::<(), rulekit::ParseError>(())
/// ```
#[derive(Clone, Debug)]
pub struct Trace<'a> {
    pub(crate) node: Option<NodeId>,
    pub(crate) kind: Option<AstKind>,
    pub(crate) expr: Cow<'a, str>,
    pub(crate) value: Val<'a>,
    pub(crate) error: Option<String>,
    pub(crate) missing: Missing<'a>,
    pub(crate) diagnostics: Vec<Diagnostic>,
    pub(crate) status: TraceStatus,
    pub(crate) active: bool,
    pub(crate) pruned: bool,
    pub(crate) children: Vec<Trace<'a>>,
}

impl Default for Trace<'_> {
    fn default() -> Self {
        Trace {
            node: None,
            kind: None,
            expr: Cow::Borrowed(""),
            value: Val::Ref(ValueRef::Null),
            error: None,
            missing: Missing::new(),
            diagnostics: Vec::new(),
            status: TraceStatus::Unknown,
            active: false,
            pruned: false,
            children: Vec::new(),
        }
    }
}

impl<'a> Trace<'a> {
    /// The AST node: of the rule, or of a macro's rule for nodes inside a
    /// macro expansion.
    pub fn node(&self) -> Option<NodeId> {
        self.node
    }

    /// The node's kind.
    pub fn kind(&self) -> Option<AstKind> {
        self.kind
    }

    /// The node's canonical expression.
    pub fn expr(&self) -> &str {
        &self.expr
    }

    /// The node's value (`Null` if it produced none).
    pub fn value(&self) -> ValueRef<'_> {
        self.value.as_ref()
    }

    /// The error message, if the node failed.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Fields the node needed but the input lacked.
    pub fn missing_fields(&self) -> &[Cow<'a, str>] {
        &self.missing
    }

    /// Comparisons that evaluated to false because they could not be made.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// How the node ended.
    pub fn status(&self) -> TraceStatus {
        self.status
    }

    /// Whether the node was evaluated.
    pub fn active(&self) -> bool {
        self.active
    }

    /// Whether the node was skipped by short-circuiting.
    pub fn pruned(&self) -> bool {
        self.pruned
    }

    /// Traces of the node's operands, arguments, or items.
    pub fn children(&self) -> &[Trace<'a>] {
        &self.children
    }

    /// A copy that owns all its data.
    pub fn into_owned(self) -> Trace<'static> {
        Trace {
            node: self.node,
            kind: self.kind,
            expr: Cow::Owned(self.expr.into_owned()),
            value: Val::Owned(self.value.into_owned()),
            error: self.error,
            missing: self
                .missing
                .into_iter()
                .map(|name| Cow::Owned(name.into_owned()))
                .collect(),
            diagnostics: self.diagnostics,
            status: self.status,
            active: self.active,
            pruned: self.pruned,
            children: self.children.into_iter().map(Trace::into_owned).collect(),
        }
    }
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

/// A trace under construction: a described node, or an unnamed group of
/// child traces (Go's `combineTrace` nodes), possibly with diagnostics.
#[derive(Debug)]
pub(crate) enum Frag<'a> {
    Node(Box<Trace<'a>>),
    Group(Vec<Trace<'a>>),
    Diagnosed(Box<(Vec<Trace<'a>>, Vec<Diagnostic>)>),
}

impl<'a> Frag<'a> {
    /// The fragment as one trace node (a group becomes an unnamed node).
    fn into_trace(self) -> Trace<'a> {
        match self {
            Frag::Node(trace) => *trace,
            Frag::Group(children) => Trace {
                children,
                ..Trace::default()
            },
            Frag::Diagnosed(group) => {
                let (children, diagnostics) = *group;
                Trace {
                    children,
                    diagnostics,
                    ..Trace::default()
                }
            }
        }
    }

    /// Whether this is an unnamed group rather than a described node.
    pub(crate) fn is_group(&self) -> bool {
        !matches!(self, Frag::Node(_))
    }

    /// The described node, if this is one.
    pub(crate) fn node(&self) -> Option<&Trace<'a>> {
        match self {
            Frag::Node(trace) => Some(trace),
            _ => None,
        }
    }
}

/// Add a diagnostic to a fragment (Go `addTraceDiagnostic`).
pub(crate) fn add_diagnostic<'a>(
    frag: Option<Frag<'a>>,
    diagnostic: Diagnostic,
) -> Option<Frag<'a>> {
    let (children, mut diagnostics) = match frag {
        None => (Vec::new(), Vec::new()),
        Some(Frag::Group(children)) => (children, Vec::new()),
        Some(Frag::Diagnosed(group)) => *group,
        Some(node @ Frag::Node(_)) => (vec![node.into_trace()], Vec::new()),
    };
    diagnostics.push(diagnostic);
    Some(Frag::Diagnosed(Box::new((children, diagnostics))))
}

/// Go `tracedRule.Eval`: describe a node, adopting the children and
/// diagnostics of the trace its evaluation produced.
pub(crate) fn wrap<'a>(meta: &'a Meta, r: &EvalResult<'a>, inner: Option<Frag<'a>>) -> Frag<'a> {
    let (children, diagnostics) = match inner {
        None => (Vec::new(), Vec::new()),
        Some(Frag::Node(trace)) => (trace.children, trace.diagnostics),
        Some(Frag::Group(children)) => (children, Vec::new()),
        Some(Frag::Diagnosed(group)) => *group,
    };
    Frag::Node(Box::new(Trace {
        node: Some(meta.id),
        kind: Some(meta.kind),
        expr: Cow::Borrowed(&meta.expr),
        value: r.value.clone(),
        error: r.error.as_ref().map(ToString::to_string),
        missing: r.missing.clone(),
        diagnostics,
        status: status(r),
        active: true,
        pruned: false,
        children,
    }))
}

/// Go `prunedTrace`.
pub(crate) fn pruned(meta: &Meta) -> Frag<'_> {
    Frag::Node(Box::new(Trace {
        node: Some(meta.id),
        kind: Some(meta.kind),
        expr: Cow::Borrowed(&meta.expr),
        status: TraceStatus::Pruned,
        pruned: true,
        ..Trace::default()
    }))
}

/// Go `combineTrace`: an unnamed group of the present children.
pub(crate) fn combine<'a>(
    children: impl IntoIterator<Item = Option<Frag<'a>>>,
) -> Option<Frag<'a>> {
    let children: Vec<Trace<'a>> = children
        .into_iter()
        .flatten()
        .map(Frag::into_trace)
        .collect();
    if children.is_empty() {
        return None;
    }
    Some(Frag::Group(children))
}
