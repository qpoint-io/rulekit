//! Output modes.

use super::{canonical, canonical_in, precedence};
use crate::ast::{Ast, NodeData, NodeId, Operator, TokenKind, TokenRef};

/// How to print a rule or AST.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PrintMode {
    /// The original expression text.
    Source,
    /// Canonical single-line output; comments are kept as `/* */`.
    Compact,
    /// Canonical output with `and`/`or` chains on separate lines, indented
    /// by the given string per group level (empty means two spaces).
    Multiline(String),
}

/// Print an AST in the given mode.
///
/// [`PrintMode::Compact`] and [`PrintMode::Multiline`] normalize spacing,
/// operator spelling (`&&` becomes `and`, `matches` becomes `=~`), and
/// parentheses; comments are kept.
///
/// ```rust
/// use rulekit::{Ast, PrintMode, format};
///
/// let ast = Ast::parse("a==1&&(b||c)")?;
/// assert_eq!(format(&ast, &PrintMode::Source), "a==1&&(b||c)");
/// assert_eq!(format(&ast, &PrintMode::Compact), "a == 1 and (b or c)");
/// assert_eq!(
///     format(&ast, &PrintMode::Multiline("  ".into())),
///     "a == 1 and (\n  b\n  or c\n)"
/// );
/// # Ok::<(), rulekit::ParseError>(())
/// ```
pub fn format(ast: &Ast, mode: &PrintMode) -> String {
    let has_comments = ast.tokens().any(|tok| {
        let trivia = tok.leading_trivia();
        trivia.contains("--") || trivia.contains("/*")
    });
    match mode {
        PrintMode::Source => ast.source().to_owned(),
        PrintMode::Multiline(indent) => {
            let indent = if indent.is_empty() {
                "  "
            } else {
                indent.as_str()
            };
            if has_comments {
                format_tokens(ast, Some(indent))
            } else {
                multiline(ast, ast.root().id(), indent, 0)
            }
        }
        PrintMode::Compact => {
            if has_comments {
                format_tokens(ast, None)
            } else {
                canonical(ast, ast.root().id())
            }
        }
    }
}

/// Re-print the token stream, keeping comments (Go `formatTokensPreservingComments`).
/// `indent` is `Some` for multiline output.
fn format_tokens(ast: &Ast, indent: Option<&str>) -> String {
    let mut f = TokenFormatter {
        out: String::new(),
        prev: None,
        depth: 0,
        at_line: true,
        multiline: indent.is_some(),
        indent: indent.unwrap_or("  "),
        groups: Vec::new(),
        space_after_comment: false,
    };
    for tok in ast.tokens() {
        f.write_trivia(tok.leading_trivia());
        if tok.kind() == TokenKind::Eof {
            break;
        }
        f.write_token(tok);
    }
    f.out.trim_end_matches([' ', '\t', '\n']).to_owned()
}

struct TokenFormatter<'a> {
    out: String,
    prev: Option<TokenKind>,
    depth: usize,
    at_line: bool,
    multiline: bool,
    indent: &'a str,
    /// For each open parenthesis: whether it groups an expression (true) or
    /// encloses call arguments (false).
    groups: Vec<bool>,
    /// Separates a token from a preceding inline comment.
    space_after_comment: bool,
}

impl TokenFormatter<'_> {
    fn write_token(&mut self, tok: TokenRef<'_>) {
        let kind = tok.kind();
        let mut closes_group = false;
        if kind == TokenKind::RParen
            && let Some(group) = self.groups.pop()
        {
            closes_group = group;
            if closes_group && self.depth > 0 {
                self.depth -= 1;
            }
        }
        if self.multiline
            && (closes_group || kind == TokenKind::And || kind == TokenKind::Or)
            && !self.at_line
        {
            self.newline();
        }

        if (self.space_after_comment && !self.at_line && kind != TokenKind::RParen)
            || self.needs_space(kind)
        {
            self.out.push(' ');
        }
        self.space_after_comment = false;

        self.out.push_str(canonical_token(tok));
        self.at_line = false;

        if kind == TokenKind::LParen {
            let group = self.prev != Some(TokenKind::Field);
            self.groups.push(group);
            if group {
                self.depth += 1;
                if self.multiline {
                    self.newline();
                }
            }
        }
        self.prev = Some(kind);
    }

    fn write_trivia(&mut self, mut trivia: &str) {
        while !trivia.is_empty() {
            let Some(idx) = next_comment(trivia) else {
                return;
            };
            trivia = &trivia[idx..];
            if trivia.starts_with("--") {
                let end = trivia.find('\n').unwrap_or(trivia.len());
                let comment = trivia[..end].trim();
                let text = comment[2..].trim();
                // Compact output is single-line, so line comments become block
                // comments unless their text would end the block early.
                if !self.multiline && !text.contains("*/") {
                    if !text.is_empty() {
                        self.write_comment(&format!("/* {text} */"));
                    }
                } else {
                    self.write_comment(comment);
                    self.newline();
                }
                if end == trivia.len() {
                    return;
                }
                trivia = &trivia[end + 1..];
                continue;
            }

            let Some(end) = trivia.find("*/") else {
                self.write_comment(trivia.trim());
                return;
            };
            let comment = trivia[..end + 2].trim();
            trivia = &trivia[end + 2..];
            if !self.multiline {
                self.write_comment(&comment.split_whitespace().collect::<Vec<_>>().join(" "));
                continue;
            }
            self.write_comment(comment);
            if trivia.contains('\n') || comment.contains('\n') {
                self.newline();
            }
        }
    }

    fn write_comment(&mut self, comment: &str) {
        if comment.is_empty() {
            return;
        }
        if !self.at_line {
            self.out.push(' ');
        }
        self.out.push_str(comment);
        self.at_line = false;
        self.space_after_comment = true;
    }

    fn newline(&mut self) {
        self.out.push('\n');
        if self.multiline {
            for _ in 0..self.depth {
                self.out.push_str(self.indent);
            }
        }
        self.at_line = true;
    }

    fn needs_space(&self, kind: TokenKind) -> bool {
        let Some(prev) = self.prev else { return false };
        if self.at_line {
            return false;
        }
        if matches!(
            kind,
            TokenKind::RParen | TokenKind::RBracket | TokenKind::Comma | TokenKind::Dot
        ) {
            return false;
        }
        if matches!(
            prev,
            TokenKind::LParen | TokenKind::LBracket | TokenKind::Dot
        ) {
            return false;
        }
        if kind == TokenKind::LParen && prev == TokenKind::Field {
            return false;
        }
        if kind == TokenKind::LBracket && matches!(prev, TokenKind::Field | TokenKind::RBracket) {
            return false;
        }
        true
    }
}

fn next_comment(trivia: &str) -> Option<usize> {
    match (trivia.find("--"), trivia.find("/*")) {
        (Some(line), Some(block)) => Some(line.min(block)),
        (line, block) => line.or(block),
    }
}

fn canonical_token<'a>(tok: TokenRef<'a>) -> &'a str {
    match tok.kind() {
        TokenKind::Not => "not",
        TokenKind::And => "and",
        TokenKind::Or => "or",
        TokenKind::Matches => "=~",
        TokenKind::Eq => "==",
        TokenKind::Ne => "!=",
        TokenKind::Gt => ">",
        TokenKind::Ge => ">=",
        TokenKind::Lt => "<",
        TokenKind::Le => "<=",
        TokenKind::Contains => "contains",
        TokenKind::In => "in",
        _ => tok.raw(),
    }
}

/// Go `formatMultiline`.
fn multiline(ast: &Ast, id: NodeId, indent: &str, depth: usize) -> String {
    let NodeData::Binary {
        op: op @ (Operator::And | Operator::Or),
        lhs,
        rhs,
        ..
    } = *ast.data(id)
    else {
        return canonical(ast, id);
    };
    if binary_op(ast, lhs) == Some(op) || binary_op(ast, rhs) == Some(op) {
        return multiline_chain(ast, id, op, indent, depth);
    }

    let left = multiline_operand(ast, lhs, indent, depth, op, false);
    let right = multiline_operand(ast, rhs, indent, depth, op, true);
    let prec = op.precedence();
    let lower = |child: NodeId| binary_op(ast, child).is_some() && precedence(ast, child) < prec;
    if lower(lhs) || lower(rhs) {
        return format!("{left} {} {right}", op.symbol());
    }
    format!("{left}\n{}{} {right}", indent.repeat(depth), op.symbol())
}

fn binary_op(ast: &Ast, id: NodeId) -> Option<Operator> {
    match ast.data(id) {
        NodeData::Binary { op, .. } => Some(*op),
        _ => None,
    }
}

fn multiline_chain(ast: &Ast, id: NodeId, op: Operator, indent: &str, depth: usize) -> String {
    let mut operands = Vec::new();
    flatten(ast, id, op, &mut operands);
    let prefix = indent.repeat(depth);
    operands
        .iter()
        .enumerate()
        .map(|(i, &operand)| {
            let formatted = multiline_operand(ast, operand, indent, depth, op, i > 0);
            if i > 0 {
                format!("{prefix}{} {formatted}", op.symbol())
            } else {
                formatted
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn flatten(ast: &Ast, id: NodeId, op: Operator, out: &mut Vec<NodeId>) {
    match *ast.data(id) {
        NodeData::Binary {
            op: node_op,
            lhs,
            rhs,
            ..
        } if node_op == op => {
            flatten(ast, lhs, op, out);
            flatten(ast, rhs, op, out);
        }
        _ => out.push(id),
    }
}

fn multiline_operand(
    ast: &Ast,
    id: NodeId,
    indent: &str,
    depth: usize,
    parent: Operator,
    right_child: bool,
) -> String {
    if let Some(op) = binary_op(ast, id) {
        if precedence(ast, id) < parent.precedence() {
            return grouped(ast, id, indent, depth);
        }
        if op == parent {
            return multiline(ast, id, indent, depth);
        }
    }
    canonical_in(ast, id, parent.precedence(), right_child)
}

fn grouped(ast: &Ast, id: NodeId, indent: &str, depth: usize) -> String {
    let inner = multiline(ast, id, indent, 0);
    let inner_indent = indent.repeat(depth + 1);
    let lines: Vec<String> = inner
        .split('\n')
        .map(|line| format!("{inner_indent}{line}"))
        .collect();
    format!("(\n{}\n{})", lines.join("\n"), indent.repeat(depth))
}
