//! Printing (port of `printAST` in `ast.go` and `format.go`).

mod format;
mod quote;
mod rewrite;

pub use format::{PrintMode, format};
pub use rewrite::{Edit, rewrite};

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

/// Compact canonical expressions of every node, indexed by `NodeId`, built in
/// one bottom-up pass (each node reuses its children's text). Equal to
/// [`canonical`] for each node.
pub(crate) fn canonical_all(ast: &Ast) -> Vec<String> {
    let mut texts: Vec<String> = Vec::with_capacity(ast.nodes.len());
    for (index, node) in ast.nodes.iter().enumerate() {
        // Children always precede their parent in the arena.
        let in_context = |child: NodeId, parent_prec: u8, right_child: bool| {
            let text = &texts[child.index()];
            let prec = precedence(ast, child);
            let unary = matches!(ast.data(child), NodeData::Unary { .. });
            if (unary && parent_prec >= Operator::Eq.precedence())
                || prec < parent_prec
                || (right_child && prec == parent_prec)
            {
                format!("({text})")
            } else {
                text.clone()
            }
        };
        let join = |ids: &[NodeId]| {
            ids.iter()
                .map(|id| texts[id.index()].as_str())
                .collect::<Vec<_>>()
                .join(", ")
        };
        let text = match node {
            NodeData::Literal { span, .. } => ast.text(*span).to_owned(),
            NodeData::Path { segments, .. } => path_string(segments),
            NodeData::Array { items, .. } => format!("[{}]", join(items)),
            NodeData::Call { name, args, .. } => format!("{}({})", ast.text(*name), join(args)),
            NodeData::Unary { operand, .. } => {
                if matches!(ast.data(*operand), NodeData::Binary { .. }) {
                    format!("not ({})", texts[operand.index()])
                } else {
                    format!("not {}", in_context(*operand, 4, true))
                }
            }
            NodeData::Binary {
                op,
                negated,
                lhs,
                rhs,
                ..
            } => {
                let prec = op.precedence();
                let negation = if *negated { "not " } else { "" };
                format!(
                    "{} {negation}{} {}",
                    in_context(*lhs, prec, false),
                    op.symbol(),
                    in_context(*rhs, prec, true)
                )
            }
        };
        debug_assert_eq!(index, texts.len());
        texts.push(text);
    }
    texts
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_all_matches_canonical() {
        for expr in [
            "a == 1 or b == 2 and not c",
            "not (a or b) and not not c == 1",
            "(not a) == false",
            "x not in [1, y] or f(a, (b or c), not d)",
            r#"a.b["c.d"][0] =~ /x/i and (p or q) and r"#,
            "a or (b or c)",
            "(a and b) and c",
            "not a == 1 and b",
        ] {
            let ast = Ast::parse(expr).unwrap();
            let texts = canonical_all(&ast);
            for (i, text) in texts.iter().enumerate() {
                assert_eq!(text, &canonical(&ast, ast.node_id(i)), "{expr}: node {i}");
            }
        }
    }
}
