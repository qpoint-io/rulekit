//! JSON form of an AST (port of `ast_json.go`).

use serde::Serialize;

use super::{Ast, NodeData, NodeId, Span, TokenKind};
use crate::print::{canonical, path_string};

/// The JSON document for a parsed AST: source, node tree, tokens (no EOF).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct JsonAst {
    pub source: String,
    pub root: JsonNode,
    pub tokens: Vec<JsonToken>,
}

/// One AST node. `id` is the tree position (`root`, `root.0`, ...); `text` is
/// the node's compact canonical expression. `operator`/`raw` are set for unary
/// and binary nodes (`negated` for `not contains`/`not matches`/`not in`),
/// `raw` is the token for literals and the name for calls, `path` is set for
/// paths.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct JsonNode {
    pub id: String,
    pub kind: &'static str,
    pub text: String,
    #[serde(skip_serializing_if = "str::is_empty")]
    pub operator: &'static str,
    #[serde(skip_serializing_if = "is_false")]
    pub negated: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub raw: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub path: String,
    pub span: JsonSpan,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<JsonNode>,
}

/// One source token; `role` is `id`, `str`, `num`, `kw`, or `pun`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct JsonToken {
    pub kind: &'static str,
    pub role: &'static str,
    pub raw: String,
    pub span: JsonSpan,
}

/// A byte span with 1-based lines and 1-based byte columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JsonSpan {
    pub start: usize,
    pub end: usize,
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl Serialize for Ast {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.json().serialize(serializer)
    }
}

impl Ast {
    /// The JSON span for a span of this AST's source.
    pub fn json_span(&self, span: Span) -> JsonSpan {
        LineIndex::new(&self.source).span(span)
    }
}

pub(super) fn build(ast: &Ast) -> JsonAst {
    let lines = LineIndex::new(&ast.source);
    let tokens = ast
        .tokens
        .iter()
        .filter(|tok| tok.kind != TokenKind::Eof)
        .map(|tok| JsonToken {
            kind: tok.kind.name(),
            role: tok.kind.role(),
            raw: ast.text(tok.span).to_owned(),
            span: lines.span(tok.span),
        })
        .collect();
    JsonAst {
        source: ast.source.clone(),
        root: node(ast, &lines, ast.root, "root".to_owned()),
        tokens,
    }
}

fn node(ast: &Ast, lines: &LineIndex, id: NodeId, json_id: String) -> JsonNode {
    let data = ast.data(id);
    let mut out = JsonNode {
        kind: ast.node(id).kind().name(),
        text: canonical(ast, id),
        operator: "",
        negated: false,
        raw: String::new(),
        path: String::new(),
        span: lines.span(data.span()),
        children: Vec::new(),
        id: String::new(),
    };
    let children: &[NodeId] = match data {
        NodeData::Literal { span, .. } => {
            out.raw = ast.text(*span).to_owned();
            &[]
        }
        NodeData::Path { segments, .. } => {
            out.path = path_string(segments);
            &[]
        }
        NodeData::Array { items, .. } => items,
        NodeData::Call { name, args, .. } => {
            out.raw = ast.text(*name).to_owned();
            args
        }
        NodeData::Unary {
            op,
            raw_op,
            operand,
            ..
        } => {
            out.operator = op.name();
            out.raw = ast.text(*raw_op).to_owned();
            std::slice::from_ref(operand)
        }
        NodeData::Binary {
            op,
            raw_op,
            negated,
            lhs,
            rhs,
            ..
        } => {
            out.operator = op.name();
            out.negated = *negated;
            out.raw = ast.text(*raw_op).to_owned();
            out.children = vec![
                node(ast, lines, *lhs, format!("{json_id}.0")),
                node(ast, lines, *rhs, format!("{json_id}.1")),
            ];
            &[]
        }
    };
    if !children.is_empty() {
        out.children = children
            .iter()
            .enumerate()
            .map(|(i, &child)| node(ast, lines, child, format!("{json_id}.{i}")))
            .collect();
    }
    out.id = json_id;
    out
}

/// Maps byte offsets to 1-based line and byte-column positions.
struct LineIndex {
    starts: Vec<usize>,
    size: usize,
}

impl LineIndex {
    fn new(source: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(
            source
                .bytes()
                .enumerate()
                .filter(|&(_, b)| b == b'\n')
                .map(|(i, _)| i + 1),
        );
        LineIndex {
            starts,
            size: source.len(),
        }
    }

    fn span(&self, span: Span) -> JsonSpan {
        let (start_line, start_column) = self.position(span.start);
        let (end_line, end_column) = self.position(span.end);
        JsonSpan {
            start: span.start,
            end: span.end,
            start_line,
            start_column,
            end_line,
            end_column,
        }
    }

    fn position(&self, offset: usize) -> (usize, usize) {
        let offset = offset.min(self.size);
        let line = self.starts.partition_point(|&start| start <= offset) - 1;
        (line + 1, offset - self.starts[line] + 1)
    }
}
