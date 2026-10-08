//! Runs the language-neutral vectors in `../testdata/vectors` (see the README
//! there for the format). This file is not part of the published package.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rulekit::value::{ObjectRef, Value, ValueRef};
use rulekit::{Ast, JsonOptions, KvInput, NoInput, Opts, PrintMode};
use serde::Deserialize;
use serde_json::Value as Json;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VectorFile {
    input_mode: Option<String>,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    name: String,
    expr: Option<String>,
    input_mode: Option<String>,
    #[serde(default, deserialize_with = "present")]
    input: Option<Json>,
    macros: Option<BTreeMap<String, String>>,
    expect: Expect,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Expect {
    #[serde(default)]
    decode_error: bool,
    parse_error: Option<ParseErrorExpect>,
    #[serde(default, deserialize_with = "present")]
    value: Option<Json>,
    error: Option<bool>,
    missing_fields: Option<Vec<String>>,
    trace: Option<TraceExpect>,
    print: Option<PrintExpect>,
    #[serde(default, deserialize_with = "present")]
    ast_json: Option<Json>,
}

impl Expect {
    fn evaluates(&self) -> bool {
        self.value.is_some()
            || self.error.is_some()
            || self.missing_fields.is_some()
            || self.trace.is_some()
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ParseErrorExpect {
    Fails(bool),
    At(ParseErrorPosition),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ParseErrorPosition {
    line: usize,
    column: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PrintExpect {
    string: Option<String>,
    compact: Option<String>,
    source: Option<String>,
    multiline_2sp: Option<String>,
    multiline_4sp: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TraceExpect {
    kind: Option<String>,
    expr: Option<String>,
    status: Option<String>,
    #[serde(default, deserialize_with = "present")]
    value: Option<Json>,
    active: Option<bool>,
    pruned: Option<bool>,
    missing_fields: Option<Vec<String>>,
    diagnostics: Option<Vec<DiagnosticExpect>>,
    children: Option<Vec<TraceExpect>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DiagnosticExpect {
    code: String,
    left_type: String,
    operator: String,
    right_type: String,
}

/// Distinguishes an explicit `null` from an absent key.
fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Json>, D::Error> {
    Json::deserialize(d).map(Some)
}

fn vector_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../testdata/vectors")
}

#[derive(Default)]
struct Report {
    passed: usize,
    failures: Vec<String>,
}

#[test]
fn vectors() {
    let mut files: Vec<PathBuf> = Vec::new();
    for entry in std::fs::read_dir(vector_dir()).expect("read vector dir") {
        let path = entry.expect("dir entry").path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_owned();
        if name == "README.md" {
            continue;
        }
        assert!(
            path.is_file() && name.ends_with(".json"),
            "unexpected entry {name:?} in vector dir"
        );
        files.push(path);
    }
    files.sort();
    assert!(!files.is_empty(), "no vector files found");

    let mut report = Report::default();
    for path in &files {
        let file_name = path
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_owned();
        let data = std::fs::read_to_string(path).expect("read vector file");
        let file: VectorFile = serde_json::from_str(&data)
            .unwrap_or_else(|err| panic!("{file_name}: invalid vector file: {err}"));
        assert!(!file.cases.is_empty(), "{file_name}: no cases");
        let mut seen = HashSet::new();
        for case in &file.cases {
            assert!(!case.name.is_empty(), "{file_name}: case without name");
            assert!(
                seen.insert(case.name.as_str()),
                "{file_name}: duplicate case name {:?}",
                case.name
            );
            let mode = case
                .input_mode
                .as_deref()
                .or(file.input_mode.as_deref())
                .unwrap_or("plain");
            assert!(
                matches!(mode, "plain" | "annotated_keys" | "typed_document"),
                "{file_name}/{}: bad input_mode {mode:?}",
                case.name
            );
            match run_case(case, mode, &mut report) {
                Ok(()) => {}
                Err(msg) => report
                    .failures
                    .push(format!("{file_name}/{}: {msg}", case.name)),
            }
        }
    }

    let summary = format!(
        "vectors: {} passed, {} failed",
        report.passed,
        report.failures.len()
    );
    eprintln!("{summary}");
    assert!(
        report.failures.is_empty(),
        "{summary}\n{}",
        report.failures.join("\n")
    );
}

fn json_options(mode: &str) -> JsonOptions {
    JsonOptions {
        annotated_keys: mode == "annotated_keys",
        typed_document: mode == "typed_document",
    }
}

fn run_case(case: &Case, mode: &str, report: &mut Report) -> Result<(), String> {
    let want = &case.expect;
    let has_expectation = want.decode_error
        || want.parse_error.is_some()
        || want.evaluates()
        || want.print.is_some()
        || want.ast_json.is_some();
    if !has_expectation {
        return Err("case has no expectations".into());
    }
    let input = match &case.input {
        Some(json) => {
            let bytes = serde_json::to_vec(json).expect("serialize input");
            let decoded = rulekit::decode_json::<()>(&bytes, json_options(mode));
            if want.decode_error {
                if case.expr.is_some() {
                    return Err("decode_error cases must not have an expr".into());
                }
                return match decoded {
                    Ok(_) => Err("expected input decode error".into()),
                    Err(_) => {
                        report.passed += 1;
                        Ok(())
                    }
                };
            }
            Some(KvInput::new(
                decoded.map_err(|err| format!("decoding input: {err}"))?,
            ))
        }
        None if want.decode_error => return Err("decode_error requires input".into()),
        None => None,
    };
    let expr = case.expr.as_deref().ok_or("case requires expr")?;

    let parsed = Ast::parse(expr)
        .map(Arc::new)
        .and_then(|ast| rulekit::compile(ast.clone()).map(|rule| (ast, rule)));
    let wants_failure = match &want.parse_error {
        Some(ParseErrorExpect::Fails(fails)) => *fails,
        Some(ParseErrorExpect::At(_)) => true,
        None => false,
    };
    if wants_failure {
        if want.evaluates() || want.print.is_some() || want.ast_json.is_some() {
            return Err("parse_error cases have no other expectations".into());
        }
        let err = match parsed {
            Ok(_) => return Err("expected parse error".into()),
            Err(err) => err,
        };
        if let Some(ParseErrorExpect::At(pos)) = &want.parse_error
            && (err.line(), err.column()) != (pos.line, pos.column)
        {
            return Err(format!(
                "parse error at {}:{}, want {}:{} ({})",
                err.line(),
                err.column(),
                pos.line,
                pos.column,
                err.message()
            ));
        }
        report.passed += 1;
        return Ok(());
    }
    let (ast, rule) = parsed.map_err(|err| format!("parse: {err}"))?;

    if let Some(print) = &want.print {
        check_print(&ast, &rule, print)?;
    }
    if let Some(expected) = &want.ast_json {
        let got = serde_json::to_value(&*ast).map_err(|err| err.to_string())?;
        if &got != expected {
            return Err(format!("ast_json mismatch\n got: {got}\nwant: {expected}"));
        }
    }
    if want.evaluates() {
        let mut builder = rulekit::Env::<()>::builder();
        for (name, source) in case.macros.iter().flatten() {
            builder = builder
                .macro_source(name.clone(), source)
                .map_err(|err| format!("macro {name:?}: {err}"))?;
        }
        let env = builder.build().map_err(|err| format!("env: {err}"))?;
        let eval = |trace: bool| {
            let opts = Opts::new(&env).with_trace(trace);
            match &input {
                Some(input) => rule.eval(&(), input, opts),
                None => rule.eval(&(), &NoInput, opts),
            }
        };
        let plain = eval(false);
        if plain.trace().is_some() {
            return Err("trace must be absent when tracing is off".into());
        }
        check_result(&plain, want).map_err(|err| format!("untraced: {err}"))?;
        let traced = eval(true);
        check_result(&traced, want).map_err(|err| format!("traced: {err}"))?;
        if let Some(expect) = &want.trace {
            let trace = traced.trace().ok_or("traced result has no trace")?;
            check_trace("trace", trace, expect)?;
        }
    }
    report.passed += 1;
    Ok(())
}

fn check_result(got: &rulekit::EvalResult<'_>, want: &Expect) -> Result<(), String> {
    let want_error = want.error.unwrap_or(false);
    match (want_error, got.error()) {
        (true, None) => return Err("expected eval error".into()),
        (false, Some(err)) => return Err(format!("eval error: {err}")),
        _ => {}
    }
    let mut want_missing = want.missing_fields.clone().unwrap_or_default();
    want_missing.sort();
    let mut got_missing: Vec<String> = got.missing_fields().map(|s| s.to_string()).collect();
    got_missing.sort();
    if want_missing != got_missing {
        return Err(format!(
            "missing_fields: got {got_missing:?}, want {want_missing:?}"
        ));
    }
    if let Some(want) = &want.value {
        let want = canonical(expected_value(want)?.as_ref())?;
        let got = canonical(got.value())?;
        if want != got {
            return Err(format!("value: got {got}, want {want}"));
        }
    }
    Ok(())
}

fn check_trace(path: &str, got: &rulekit::Trace, want: &TraceExpect) -> Result<(), String> {
    let fail = |what: &str, got: &dyn std::fmt::Debug, want: &dyn std::fmt::Debug| {
        Err(format!("{path}.{what}: got {got:?}, want {want:?}"))
    };
    if let Some(kind) = &want.kind {
        let got_kind = got.kind().map_or("", |k| k.name());
        if got_kind != kind {
            return fail("kind", &got_kind, kind);
        }
    }
    if let Some(expr) = &want.expr
        && got.expr() != expr
    {
        return fail("expr", &got.expr(), expr);
    }
    if let Some(status) = &want.status
        && got.status().name() != status
    {
        return fail("status", &got.status().name(), status);
    }
    if let Some(value) = &want.value {
        let want_value = canonical(expected_value(value)?.as_ref())?;
        let got_value = canonical(got.value())?;
        if want_value != got_value {
            return fail("value", &got_value, &want_value);
        }
    }
    if let Some(active) = want.active
        && got.active() != active
    {
        return fail("active", &got.active(), &active);
    }
    if let Some(pruned) = want.pruned
        && got.pruned() != pruned
    {
        return fail("pruned", &got.pruned(), &pruned);
    }
    if let Some(missing) = &want.missing_fields {
        let (mut a, mut b) = (
            got.missing_fields()
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>(),
            missing.clone(),
        );
        a.sort();
        b.sort();
        if a != b {
            return fail("missing_fields", &a, &b);
        }
    }
    if let Some(diagnostics) = &want.diagnostics {
        let got_diags: Vec<_> = got
            .diagnostics()
            .iter()
            .map(|d| (d.code.name(), d.left_type, d.operator, d.right_type))
            .collect();
        let want_diags: Vec<_> = diagnostics
            .iter()
            .map(|d| {
                (
                    d.code.as_str(),
                    d.left_type.as_str(),
                    d.operator.as_str(),
                    d.right_type.as_str(),
                )
            })
            .collect();
        if got_diags != want_diags {
            return fail("diagnostics", &got_diags, &want_diags);
        }
        if got.diagnostics().iter().any(|d| d.message.is_empty()) {
            return Err(format!("{path}.diagnostics: empty message"));
        }
    }
    if let Some(children) = &want.children {
        if got.children().len() != children.len() {
            return fail("children.len", &got.children().len(), &children.len());
        }
        for (i, (g, w)) in got.children().iter().zip(children).enumerate() {
            check_trace(&format!("{path}.children[{i}]"), g, w)?;
        }
    }
    Ok(())
}

/// Decode a value in the vector shorthand: JSON scalars, arrays, and objects,
/// with `{"$type": ...}` objects decoded as typed JSON.
fn expected_value(json: &Json) -> Result<Value, String> {
    Ok(match json {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Bool(*b),
        Json::String(s) => Value::String(s.clone()),
        Json::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Int(i)
            } else if let Some(u) = n.as_u64() {
                Value::Uint(u)
            } else {
                Value::Float(n.as_f64().ok_or("bad number")?)
            }
        }
        Json::Array(items) => {
            Value::Array(items.iter().map(expected_value).collect::<Result<_, _>>()?)
        }
        Json::Object(map) if map.contains_key("$type") => {
            let doc = serde_json::to_vec(&serde_json::json!({ "v": json })).expect("serialize");
            let opts = JsonOptions {
                typed_document: true,
                ..Default::default()
            };
            let mut kv = rulekit::decode_json::<()>(&doc, opts)
                .map_err(|err| format!("expected value: {err}"))?;
            match kv.remove("v") {
                Some(rulekit::KvEntry::Value(v)) => v,
                _ => return Err("expected value: not a value".into()),
            }
        }
        Json::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| Ok((k.clone(), expected_value(v)?)))
                .collect::<Result<_, String>>()?,
        ),
    })
}

/// A canonical typed form that keeps type distinctions.
fn canonical(value: ValueRef<'_>) -> Result<Json, String> {
    let typed = |t: &str, v: String| serde_json::json!({ "$type": t, "value": v });
    Ok(match value {
        ValueRef::Null => Json::Null,
        ValueRef::Bool(b) => Json::Bool(b),
        ValueRef::Str(s) => Json::String(s.to_owned()),
        ValueRef::Int(n) => typed("int64", n.to_string()),
        ValueRef::Uint(n) => typed("uint64", n.to_string()),
        ValueRef::Float(n) => typed("float64", format!("{n:?}")),
        ValueRef::Ip(_) | ValueRef::Cidr(_) | ValueRef::Mac(_) | ValueRef::Url(_) => typed(
            value.type_name(),
            value.text().expect("text form").to_string(),
        ),
        ValueRef::Bytes(b) => typed("bytes", b.iter().map(|b| format!("{b:02x}")).collect()),
        ValueRef::Array(items) => serde_json::json!({
            "$type": "array",
            "value": items.iter().map(canonical).collect::<Result<Vec<_>, _>>()?,
        }),
        ValueRef::Object(ObjectRef::Map(map)) => {
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                out.insert(k.clone(), canonical(v.as_ref())?);
            }
            serde_json::json!({ "$type": "object", "value": out })
        }
        ValueRef::Query(_)
        | ValueRef::Regex(_)
        | ValueRef::UrlText(_)
        | ValueRef::Object(ObjectRef::Opaque | ObjectRef::Source(_)) => {
            return Err(format!("value has no vector representation: {value:?}"));
        }
    })
}

fn check_print(ast: &Ast, rule: &rulekit::Rule, want: &PrintExpect) -> Result<(), String> {
    let canonical = rule.to_string();
    if ast.to_string() != canonical {
        return Err(format!(
            "AST string {:?} != rule string {canonical:?}",
            ast.to_string()
        ));
    }
    if let Some(want) = &want.string {
        if want != &canonical {
            return Err(format!("string: got {canonical:?}, want {want:?}"));
        }
        let again =
            rulekit::parse(&canonical).map_err(|err| format!("string output must parse: {err}"))?;
        if again.to_string() != canonical {
            return Err("string output must re-parse to itself".into());
        }
    }
    let checks = [
        ("source", PrintMode::Source, &want.source, false),
        ("compact", PrintMode::Compact, &want.compact, true),
        (
            "multiline_2sp",
            PrintMode::Multiline("  ".into()),
            &want.multiline_2sp,
            true,
        ),
        (
            "multiline_4sp",
            PrintMode::Multiline("    ".into()),
            &want.multiline_4sp,
            true,
        ),
    ];
    for (label, mode, want, reparse) in checks {
        let Some(want) = want else { continue };
        let got = rulekit::format(ast, &mode);
        if &got != want {
            return Err(format!("{label}: got {got:?}, want {want:?}"));
        }
        if rule.print(&mode) != got {
            return Err(format!("{label}: compiled rule prints differently"));
        }
        if reparse {
            let again =
                rulekit::parse(&got).map_err(|err| format!("{label} output must parse: {err}"))?;
            if again.to_string() != canonical {
                return Err(format!(
                    "{label} output must re-parse to the same expression"
                ));
            }
        }
    }
    Ok(())
}
