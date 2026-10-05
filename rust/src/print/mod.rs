//! Printing (port of `printAST` in `ast.go` and `format.go`).

mod format;
mod quote;

pub use format::{PrintMode, format};

use crate::ast::{Ast, NodeData, NodeId, Operator, Segment};

/// Precedence of a node for printing (Go `astPrecedence`).
pub(crate) fn precedence(ast: &Ast, id: NodeId) -> u8 {
    match ast.data(id) {
        NodeData::Unary { .. } => 4,
        NodeData::Binary { op, .. } => op.precedence(),
        _ => 5,
    }
}

/// Compact canonical expression for a node (Go `printAST`).
pub(crate) fn canonical(ast: &Ast, id: NodeId) -> String {
    canonical_in(ast, id, 0, false)
}

/// Go `printASTWithParent`.
pub(crate) fn canonical_in(ast: &Ast, id: NodeId, parent_prec: u8, right_child: bool) -> String {
    let prec = precedence(ast, id);
    let out = match ast.data(id) {
        NodeData::Literal { span, .. } => ast.text(*span).to_owned(),
        NodeData::Path { segments, .. } => path_string(segments),
        NodeData::Array { items, .. } => format!("[{}]", join(ast, items)),
        NodeData::Call { name, args, .. } => format!("{}({})", ast.text(*name), join(ast, args)),
        NodeData::Unary { operand, .. } => {
            let operand_text = if matches!(ast.data(*operand), NodeData::Binary { .. }) {
                format!("({})", canonical(ast, *operand))
            } else {
                canonical_in(ast, *operand, prec, true)
            };
            let out = format!("not {operand_text}");
            // not binds looser than comparisons, so it needs parentheses as a
            // comparison operand.
            if parent_prec >= Operator::Eq.precedence() {
                return format!("({out})");
            }
            out
        }
        NodeData::Binary {
            op,
            negated,
            lhs,
            rhs,
            ..
        } => {
            let negation = if *negated { "not " } else { "" };
            format!(
                "{} {negation}{} {}",
                canonical_in(ast, *lhs, prec, false),
                op.symbol(),
                canonical_in(ast, *rhs, prec, true)
            )
        }
    };
    if prec < parent_prec || (right_child && prec == parent_prec) {
        return format!("({out})");
    }
    out
}

fn join(ast: &Ast, ids: &[NodeId]) -> String {
    ids.iter()
        .map(|&id| canonical(ast, id))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Render path segments in rule syntax (Go `pathString(segments, true)`):
/// keys written with brackets, empty keys, and keys containing `.` are
/// bracketed and quoted; indexes are `[n]`.
pub(crate) fn path_string(segments: &[Segment]) -> String {
    let mut out = String::new();
    for (i, segment) in segments.iter().enumerate() {
        match segment {
            Segment::Index(index) => {
                out.push('[');
                out.push_str(&index.to_string());
                out.push(']');
            }
            Segment::Key { key, bracket } => {
                if *bracket || key.is_empty() || key.contains('.') {
                    out.push('[');
                    quote::quote_into(&mut out, key);
                    out.push(']');
                } else {
                    if i > 0 {
                        out.push('.');
                    }
                    out.push_str(key);
                }
            }
        }
    }
    out
}
