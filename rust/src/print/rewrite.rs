//! Source-preserving rewrites.

use super::{PrintMode, format};
use crate::ast::{Ast, NodeId};
use crate::error::Error;

/// One replacement for [`rewrite`]: the node `target` of the rewritten AST is
/// replaced by `replacement`, printed in the rewrite's mode.
#[derive(Clone, Copy, Debug)]
pub struct Edit<'a> {
    /// A node of the AST being rewritten.
    pub target: NodeId,
    /// The expression to put in its place.
    pub replacement: &'a Ast,
}

/// Replace AST nodes while keeping the rest of the source text, including
/// comments and spacing, as written.
///
/// Each replacement is printed in `mode` and inserted as is: it is not
/// parenthesized, so write the parentheses in the replacement's source when
/// its position needs them. With no edits, the result is the original
/// source.
///
/// # Errors
///
/// [`Error::Rewrite`] if edits overlap or a target is not a node of `ast`.
///
/// ```rust
/// use rulekit::{Ast, Edit, PrintMode, rewrite};
///
/// let ast = Ast::parse("a == 1   -- first check\nand b")?;
/// let target = ast.root().children()[0].id(); // `a == 1`
/// let replacement = Ast::parse("c   ==  2")?;
///
/// let edits = [Edit { target, replacement: &replacement }];
/// let out = rewrite(&ast, &edits, &PrintMode::Compact)?;
/// assert_eq!(out, "c == 2   -- first check\nand b");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn rewrite(ast: &Ast, edits: &[Edit<'_>], mode: &PrintMode) -> Result<String, Error> {
    if edits.is_empty() {
        return Ok(ast.source().to_owned());
    }
    let mut replacements = Vec::with_capacity(edits.len());
    for edit in edits {
        if edit.target.index() >= ast.nodes.len() {
            return Err(Error::Rewrite(
                "edit target is not a node of this AST".into(),
            ));
        }
        replacements.push((ast.data(edit.target).span(), format(edit.replacement, mode)));
    }
    replacements.sort_by_key(|(span, _)| span.start);

    let source = ast.source();
    let mut out = String::with_capacity(source.len());
    let mut pos = 0;
    for (span, replacement) in &replacements {
        if span.start < pos {
            return Err(Error::Rewrite("edits must not overlap".into()));
        }
        out.push_str(&source[pos..span.start]);
        out.push_str(replacement);
        pos = span.end;
    }
    out.push_str(&source[pos..]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(ast: &Ast, text: &str) -> NodeId {
        fn walk(node: crate::ast::NodeRef<'_>, text: &str) -> Option<NodeId> {
            if node.to_string() == text {
                return Some(node.id());
            }
            node.children()
                .into_iter()
                .find_map(|child| walk(child, text))
        }
        walk(ast.root(), text).expect("node")
    }

    #[test]
    fn rewrites_preserving_source() {
        let ast = Ast::parse("a == 1   -- keep\nAND  b in [1,2]").unwrap();
        let replacement = Ast::parse("c   ==  2").unwrap();
        let edits = [Edit {
            target: find(&ast, "a == 1"),
            replacement: &replacement,
        }];
        let out = rewrite(&ast, &edits, &PrintMode::Compact).unwrap();
        assert_eq!(out, "c == 2   -- keep\nAND  b in [1,2]");

        let two = Ast::parse("x").unwrap();
        let edits = [
            Edit {
                target: find(&ast, "[1, 2]"),
                replacement: &replacement,
            },
            Edit {
                target: find(&ast, "b"),
                replacement: &two,
            },
        ];
        let out = rewrite(&ast, &edits, &PrintMode::Source).unwrap();
        assert_eq!(out, "a == 1   -- keep\nAND  x in c   ==  2");
        assert_eq!(
            rewrite(&ast, &[], &PrintMode::Compact).unwrap(),
            ast.source()
        );
    }

    #[test]
    fn rejects_overlap_and_foreign_nodes() {
        let ast = Ast::parse("a == 1 and b").unwrap();
        let r = Ast::parse("x").unwrap();
        let edits = [
            Edit {
                target: ast.root().id(),
                replacement: &r,
            },
            Edit {
                target: find(&ast, "a"),
                replacement: &r,
            },
        ];
        assert!(rewrite(&ast, &edits, &PrintMode::Compact).is_err());
        let big = Ast::parse("a == 1 and b == 2 and c == 3 and d").unwrap();
        let foreign = [Edit {
            target: find(&big, "d"),
            replacement: &r,
        }];
        assert!(rewrite(&ast, &foreign, &PrintMode::Compact).is_err());
    }
}
