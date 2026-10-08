//! Demo-local WASM bridge: the Vue app's parse, format, rewrite, delete, and
//! eval calls, served by the Rust rulekit crate. Every export takes strings
//! and returns a JSON document with the shapes in `src/lib/rulekit.ts`.

use std::fmt::Display;
use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use rulekit::ast::{
    AstKind, JsonNode, JsonSpan, JsonToken, NodeRef, Operator, TokenKind, TokenRef,
};
use rulekit::value::{ObjectRef, ValueRef};
use rulekit::{Ast, Diagnostic, Edit, EvalResult, JsonOptions, KvInput, Opts, PrintMode, Trace};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use wasm_bindgen::prelude::wasm_bindgen;

#[wasm_bindgen]
pub fn parse(source: &str) -> String {
    to_json(&parse_rule(source))
}

#[wasm_bindgen]
pub fn format(source: &str, mode: &str) -> String {
    to_json(&format_rule(source, mode))
}

#[wasm_bindgen]
pub fn rewrite(source: &str, edit: &str) -> String {
    to_json(&rewrite_rule(source, edit))
}

#[wasm_bindgen(js_name = deleteNode)]
pub fn delete_node(source: &str, target: &str) -> String {
    to_json(&delete_rule_node(source, target))
}

#[wasm_bindgen(js_name = evalRule)]
pub fn eval_rule(source: &str, input_json: &str) -> String {
    to_json(&eval_rule_json(source, input_json))
}

#[derive(Default, Serialize)]
struct ParseResponse {
    ok: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    source: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    compact: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    multiline: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    ast: Option<JsonNode>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tokens: Vec<JsonToken>,
    #[serde(skip_serializing_if = "String::is_empty")]
    error: String,
}

impl ParseResponse {
    fn failed(source: &str, err: impl Display) -> Self {
        ParseResponse {
            source: source.to_owned(),
            error: err.to_string(),
            ..ParseResponse::default()
        }
    }
}

#[derive(Default, Serialize)]
struct SourceResponse {
    ok: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    ast: Option<JsonNode>,
    #[serde(skip_serializing_if = "String::is_empty")]
    error: String,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct EvalResponse {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<Json>,
    #[serde(skip_serializing_if = "str::is_empty")]
    status: &'static str,
    #[serde(skip_serializing_if = "String::is_empty")]
    error: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    missing_fields: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    trace: Option<TraceDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ast: Option<JsonNode>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TraceDto {
    #[serde(skip_serializing_if = "str::is_empty")]
    kind: &'static str,
    #[serde(skip_serializing_if = "String::is_empty")]
    expr: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<Json>,
    #[serde(skip_serializing_if = "String::is_empty")]
    error: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    missing_fields: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    diagnostics: Vec<DiagnosticDto>,
    status: &'static str,
    #[serde(skip_serializing_if = "is_false")]
    active: bool,
    #[serde(skip_serializing_if = "is_false")]
    pruned: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    span: Option<JsonSpan>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    children: Vec<TraceDto>,
}

/// The demo's `Diagnostic` type uses the Go struct's field names.
#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct DiagnosticDto {
    code: &'static str,
    message: String,
    left_type: &'static str,
    operator: &'static str,
    right_type: &'static str,
}

/// A node chosen in the editor: by tree id (`root.0.1`), or else by span.
#[derive(Default, Deserialize)]
#[serde(default)]
struct NodeTarget {
    id: String,
    start: usize,
    end: usize,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct EditRequest {
    target: NodeTarget,
    replacement: String,
    /// `operator` replaces a binary operator's spelling; anything else
    /// replaces the whole node with the parsed `replacement`.
    kind: String,
    mode: String,
}

struct Found<'a> {
    node: NodeRef<'a>,
    parent: Option<NodeRef<'a>>,
    index: usize,
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn to_json(value: &impl Serialize) -> String {
    serde_json::to_string(value).unwrap_or_else(|err| {
        serde_json::json!({ "ok": false, "error": err.to_string() }).to_string()
    })
}

fn parse_rule(source: &str) -> ParseResponse {
    let ast = match Ast::parse(source) {
        Ok(ast) => ast,
        Err(err) => return ParseResponse::failed(source, err),
    };
    let doc = ast.json();
    ParseResponse {
        ok: true,
        source: source.to_owned(),
        compact: rulekit::format(&ast, &PrintMode::Compact),
        multiline: rulekit::format(&ast, &PrintMode::Multiline("  ".to_owned())),
        ast: Some(doc.root),
        tokens: doc.tokens,
        error: String::new(),
    }
}

fn format_rule(source: &str, mode: &str) -> SourceResponse {
    let ast = match Ast::parse(source) {
        Ok(ast) => ast,
        Err(err) => {
            return SourceResponse {
                error: err.to_string(),
                ..SourceResponse::default()
            };
        }
    };
    let out = rulekit::format(&ast, &print_mode(mode));
    let parsed = parse_rule(&out);
    SourceResponse {
        ok: parsed.ok,
        source: out,
        ast: parsed.ast,
        error: parsed.error,
    }
}

fn rewrite_rule(source: &str, raw: &str) -> ParseResponse {
    let req: EditRequest = match serde_json::from_str(raw) {
        Ok(req) => req,
        Err(err) => return ParseResponse::failed(source, err),
    };
    let ast = match Ast::parse(source) {
        Ok(ast) => ast,
        Err(err) => return ParseResponse::failed(source, err),
    };
    let Some(found) = find_node(&ast, &req.target) else {
        return ParseResponse::failed(source, "target node not found");
    };
    let out = if req.kind == "operator" {
        rewrite_operator(source, found.node, &req.replacement)
    } else {
        Ast::parse(&req.replacement)
            .map_err(|err| err.to_string())
            .and_then(|replacement| {
                let edit = Edit {
                    target: found.node.id(),
                    replacement: &replacement,
                };
                rulekit::rewrite(&ast, &[edit], &print_mode(&req.mode))
                    .map_err(|err| err.to_string())
            })
    };
    match out {
        Ok(out) => parse_rule(&out),
        Err(err) => ParseResponse::failed(source, err),
    }
}

fn delete_rule_node(source: &str, raw: &str) -> ParseResponse {
    let target: NodeTarget = match serde_json::from_str(raw) {
        Ok(target) => target,
        Err(err) => return ParseResponse::failed(source, err),
    };
    let ast = match Ast::parse(source) {
        Ok(ast) => ast,
        Err(err) => return ParseResponse::failed(source, err),
    };
    let Some(found) = find_node(&ast, &target) else {
        return ParseResponse::failed(source, "target node not found");
    };
    let Some(parent) = found.parent else {
        return ParseResponse::failed(source, "cannot delete the root expression");
    };
    let children = parent.children();
    let replacement = match parent.kind() {
        AstKind::Binary => {
            if !matches!(parent.operator(), Some(Operator::And | Operator::Or)) {
                return ParseResponse::failed(source, "cannot delete part of a comparison");
            }
            node_source(&ast, source, children[1 - found.index], parent.span())
        }
        AstKind::Unary => node_source(&ast, source, children[0], parent.span()),
        AstKind::Array => {
            if children.len() <= 1 {
                return ParseResponse::failed(source, "array requires at least one value");
            }
            let items: Vec<String> = children
                .iter()
                .enumerate()
                .filter(|&(i, _)| i != found.index)
                .map(|(_, &child)| node_source(&ast, source, child, parent.span()))
                .collect();
            format!("[{}]", items.join(", "))
        }
        _ => return ParseResponse::failed(source, "cannot delete this node"),
    };
    // Splice text rather than reparse the replacement on its own: spans
    // exclude a group's own parentheses, so the surviving sibling of
    // `(a and b) or c` is `a and b)`, which only parses back in place.
    let span = parent.span();
    let mut out = String::with_capacity(source.len());
    out.push_str(&source[..span.start]);
    out.push_str(&replacement);
    out.push_str(&source[span.end..]);
    parse_rule(&out)
}

fn eval_rule_json(source: &str, input_json: &str) -> EvalResponse {
    let ast = match Ast::parse(source) {
        Ok(ast) => Arc::new(ast),
        Err(err) => {
            return EvalResponse {
                error: err.to_string(),
                ..EvalResponse::default()
            };
        }
    };
    let failed = |error: String| EvalResponse {
        error,
        ast: Some(ast.json().root),
        ..EvalResponse::default()
    };
    let rule = match rulekit::compile(Arc::clone(&ast)) {
        Ok(rule) => rule,
        Err(err) => return failed(err.to_string()),
    };
    let input = match rulekit::decode_json::<()>(input_json.as_bytes(), JsonOptions::default()) {
        Ok(kv) => KvInput::new(kv),
        Err(err) => return failed(format!("input json: {err}")),
    };
    let res = rule.eval(&(), &input, Opts::default().with_trace(true));
    EvalResponse {
        ok: res.error().is_none(),
        value: present(res.value()),
        status: result_status(&res),
        error: res.error().map(ToString::to_string).unwrap_or_default(),
        missing_fields: res.missing_fields().map(str::to_owned).collect(),
        trace: res.trace().map(|trace| trace_dto(&ast, trace)),
        ast: Some(ast.json().root),
    }
}

fn result_status(res: &EvalResult<'_>) -> &'static str {
    if res.error().is_some() {
        "error"
    } else if res.missing_fields().next().is_some() {
        "missing"
    } else if res.pass() {
        "passed"
    } else if res.fail() {
        "failed"
    } else {
        "unknown"
    }
}

fn print_mode(mode: &str) -> PrintMode {
    match mode {
        "multiline" => PrintMode::Multiline("  ".to_owned()),
        "source" => PrintMode::Source,
        _ => PrintMode::Compact,
    }
}

fn trace_dto(ast: &Ast, trace: &Trace<'_>) -> TraceDto {
    TraceDto {
        kind: trace.kind().map_or("", AstKind::name),
        expr: trace.expr().to_owned(),
        value: present(trace.value()),
        error: trace.error().unwrap_or_default().to_owned(),
        missing_fields: trace
            .missing_fields()
            .iter()
            .map(|f| f.to_string())
            .collect(),
        diagnostics: trace.diagnostics().iter().map(diagnostic_dto).collect(),
        status: trace.status().name(),
        active: trace.active(),
        pruned: trace.pruned(),
        // The demo registers no macros, so every traced node is in `ast`.
        span: trace.node().map(|id| ast.json_span(ast.node(id).span())),
        children: trace
            .children()
            .iter()
            .map(|child| trace_dto(ast, child))
            .collect(),
    }
}

fn diagnostic_dto(d: &Diagnostic) -> DiagnosticDto {
    DiagnosticDto {
        code: d.code.name(),
        message: d.message.clone(),
        left_type: d.left_type,
        operator: d.operator,
        right_type: d.right_type,
    }
}

/// A result or trace value, omitted when null.
fn present(value: ValueRef<'_>) -> Option<Json> {
    match value {
        ValueRef::Null => None,
        value => Some(value_json(value)),
    }
}

/// JSON for a rule value: typed network values and URLs as their text,
/// bytes as base64, regexes as their pattern.
fn value_json(value: ValueRef<'_>) -> Json {
    match value {
        ValueRef::Null | ValueRef::Object(ObjectRef::Opaque | ObjectRef::Source(_)) => Json::Null,
        ValueRef::Bool(b) => b.into(),
        ValueRef::Int(n) => n.into(),
        ValueRef::Uint(n) => n.into(),
        ValueRef::Float(n) => n.into(),
        ValueRef::Str(s) | ValueRef::Query(s) => s.into(),
        ValueRef::Bytes(b) => BASE64.encode(b).into(),
        ValueRef::Ip(_) | ValueRef::Cidr(_) | ValueRef::Mac(_) | ValueRef::Url(_) => {
            value.text().map_or(Json::Null, |text| Json::from(&*text))
        }
        ValueRef::UrlText(_) => value_json(value.to_owned().as_ref()),
        ValueRef::Regex(re) => re.as_str().into(),
        ValueRef::Array(items) => items.iter().map(value_json).collect(),
        ValueRef::Object(ObjectRef::Map(map)) => map
            .iter()
            .map(|(key, item)| (key.clone(), value_json(item.as_ref())))
            .collect::<serde_json::Map<_, _>>()
            .into(),
    }
}

fn find_node<'a>(ast: &'a Ast, target: &NodeTarget) -> Option<Found<'a>> {
    fn walk<'a>(
        node: NodeRef<'a>,
        id: &str,
        parent: Option<NodeRef<'a>>,
        index: usize,
        target: &NodeTarget,
    ) -> Option<Found<'a>> {
        let span = node.span();
        if (!target.id.is_empty() && id == target.id)
            || (span.start == target.start && span.end == target.end)
        {
            return Some(Found {
                node,
                parent,
                index,
            });
        }
        node.children()
            .into_iter()
            .enumerate()
            .find_map(|(i, child)| walk(child, &format!("{id}.{i}"), Some(node), i, target))
    }
    walk(ast.root(), "root", None, 0, target)
}

/// Replace a binary operator's spelling in `source`, keeping its operands
/// and everything around them as written.
fn rewrite_operator(source: &str, node: NodeRef<'_>, next: &str) -> Result<String, String> {
    let children = node.children();
    let (AstKind::Binary, [left, right], Some(raw)) =
        (node.kind(), children.as_slice(), node.raw_operator())
    else {
        return Err("operator rewrite requires a binary node".to_owned());
    };
    let (start, end) = (left.span().end, right.span().start);
    let between = source
        .get(start..end)
        .ok_or("operator span is outside source")?;
    let at = start + between.find(raw).ok_or("operator token not found")?;
    let next = operator_spelling(next).unwrap_or(next);
    Ok(format!(
        "{}{next}{}",
        &source[..at],
        &source[at + raw.len()..]
    ))
}

/// Rule text for the operator names the demo's editor uses: the AST JSON
/// names, plus `not_<op>` for a negated operator.
fn operator_spelling(name: &str) -> Option<&'static str> {
    Some(match name {
        "eq" => "==",
        "ne" => "!=",
        "gt" => ">",
        "ge" => ">=",
        "lt" => "<",
        "le" => "<=",
        "matches" => "=~",
        "not_contains" => "not contains",
        "not_matches" => "not =~",
        "not_in" => "not in",
        _ => return None,
    })
}

/// The source text of `node`, widened within `bounds` to balance parentheses.
/// Spans exclude a group's own parentheses: in `(a and b) or c` the left
/// operand spans `a and b`, and the chain `(x) or (y)` spans `x) or (y`.
fn node_source(ast: &Ast, source: &str, node: NodeRef<'_>, bounds: rulekit::ast::Span) -> String {
    let span = node.span();
    let parens = |tok: &TokenRef<'_>| match tok.kind() {
        TokenKind::LParen => 1,
        TokenKind::RParen => -1,
        _ => 0,
    };
    let inside = |t: &TokenRef<'_>| t.span().start >= bounds.start && t.span().end <= bounds.end;
    let (mut depth, mut lowest) = (0i32, 0i32);
    for tok in ast
        .tokens()
        .filter(|t| t.span().start >= span.start && t.span().end <= span.end)
    {
        depth += parens(&tok);
        lowest = lowest.min(depth);
    }
    // Pull in the `(`s that unmatched `)`s close, then the `)`s that close
    // unmatched `(`s, as far as the enclosing node reaches.
    let mut start = span.start;
    let (mut need, mut skip) = (-lowest, 0);
    let before: Vec<_> = ast
        .tokens()
        .filter(|t| inside(t) && t.span().end <= span.start)
        .collect();
    for tok in before.iter().rev() {
        if need == 0 {
            break;
        }
        match parens(tok) {
            -1 => skip += 1,
            1 if skip > 0 => skip -= 1,
            1 => {
                need -= 1;
                start = tok.span().start;
            }
            _ => {}
        }
    }
    let mut end = span.end;
    let (mut need, mut skip) = (depth - lowest, 0);
    for tok in ast
        .tokens()
        .filter(|t| inside(t) && t.span().start >= span.end)
    {
        if need == 0 {
            break;
        }
        match parens(&tok) {
            1 => skip += 1,
            -1 if skip > 0 => skip -= 1,
            -1 => {
                need -= 1;
                end = tok.span().end;
            }
            _ => {}
        }
    }
    // Parentheses around the node itself sit just outside its span; take
    // the ones the enclosing node covers (the rest stay in the source).
    let tokens: Vec<_> = ast.tokens().filter(inside).collect();
    loop {
        let prev = tokens.iter().rev().find(|t| t.span().end <= start);
        let next = tokens.iter().find(|t| t.span().start >= end);
        let opens = prev.filter(|t| t.kind() == TokenKind::LParen);
        let closes = next.filter(|t| t.kind() == TokenKind::RParen);
        if opens.is_none() && closes.is_none() {
            break;
        }
        if let Some(t) = opens {
            start = t.span().start;
        }
        if let Some(t) = closes {
            end = t.span().end;
        }
    }
    source
        .get(start..end)
        .map_or_else(|| node.to_string(), str::to_owned)
}
