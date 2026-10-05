//! Benchmark cases shared by the timing and allocation benches. They mirror
//! the Go benchmarks in `rule_test.go` and `compare_test.go` by name.

#![allow(dead_code)]

use rulekit::value::{Ip, Map, Url, Value, ValueRef};
use std::collections::HashMap;

use rulekit::{Env, FuncSchema, Function, KvEntry, KvInput, Lazy, Opts, Rule};

/// One `BenchmarkEval`-style case: a rule, its input, and its environment.
pub struct EvalCase {
    pub name: &'static str,
    pub rule: Rule,
    pub input: KvInput,
    pub env: Env,
    pub trace: bool,
}

impl EvalCase {
    pub fn eval(&self) -> bool {
        let opts = Opts::new(&self.env).with_trace(self.trace);
        let result = self.rule.eval(&(), &self.input, opts);
        assert!(
            result.error().is_none(),
            "{}: {:?}",
            self.name,
            result.error()
        );
        result.pass()
    }
}

fn s(v: &str) -> Value {
    Value::String(v.to_owned())
}

fn object(entries: Vec<(&str, Value)>) -> Value {
    Value::Object(
        entries
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect(),
    )
}

fn input(entries: Vec<(&str, Value)>) -> KvInput {
    KvInput::from_values(
        entries
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect::<Map<Value>>(),
    )
}

fn ip(v: &str) -> Value {
    Value::Ip(Ip::parse(v).expect("ip"))
}

fn case(name: &'static str, expr: &str, input: KvInput) -> EvalCase {
    EvalCase {
        name,
        rule: rulekit::parse(expr).expect("parse"),
        input,
        env: Env::new(),
        trace: false,
    }
}

const FIRST: &str =
    r#"tags == "db-svc" or domain matches /example\.com$/ or destination.ip in 192.168.0.0/16"#;
const FULL: &str = r#"tags == "db-svc" or domain matches /example\.com$/ or process.uid == 0 or destination.ip in 192.168.0.0/16"#;

fn traversal(dst: &str) -> KvInput {
    input(vec![
        ("tags", s("other")),
        ("domain", s("qpoint.io")),
        ("process", object(vec![("uid", Value::Int(1000))])),
        ("destination", object(vec![("ip", ip(dst))])),
    ])
}

/// Go `BenchmarkEval`.
pub fn eval_cases() -> Vec<EvalCase> {
    let mut macro_case = case(
        "macro",
        r#"is_internal() and user != "root""#,
        input(vec![("ip", ip("172.16.0.1")), ("user", s("api"))]),
    );
    let mut custom = case(
        "function_borrowed_str",
        r#"has_prefix(path, "/api")"#,
        input(vec![("path", s("/api/v1"))]),
    );
    custom.env = Env::builder().function(has_prefix()).build().unwrap();
    macro_case.env = Env::builder()
        .macro_source("is_internal", "ip in 172.16.0.0/16")
        .unwrap()
        .build()
        .unwrap();
    vec![
        case(
            "short_circuit_first_branch_string",
            FIRST,
            input(vec![("tags", s("db-svc"))]),
        ),
        case(
            "short_circuit_first_branch_string_slice",
            FIRST,
            input(vec![(
                "tags",
                Value::Array(vec![s("db-svc"), s("internal-vlan")]),
            )]),
        ),
        case(
            "full_traversal_last_branch_pass",
            FULL,
            traversal("192.168.2.37"),
        ),
        case("full_traversal_no_match", FULL, traversal("10.0.0.1")),
        case(
            "nested_path_number",
            "process.uid != 0 and destination.port <= 1023",
            input(vec![
                ("process", object(vec![("uid", Value::Int(1000))])),
                ("destination", object(vec![("port", Value::Int(443))])),
            ]),
        ),
        case(
            "bracket_path",
            r#"request.headers["user-agent"] == "curl""#,
            input(vec![(
                "request",
                object(vec![("headers", object(vec![("user-agent", s("curl"))]))]),
            )]),
        ),
        case(
            "array_index_path",
            r#"items[0].name == "first""#,
            input(vec![(
                "items",
                Value::Array(vec![
                    object(vec![("name", s("first"))]),
                    object(vec![("name", s("second"))]),
                ]),
            )]),
        ),
        case(
            "regex",
            r"domain matches /example\.com$/",
            input(vec![("domain", s("api.example.com"))]),
        ),
        case(
            "ip_cidr",
            "destination.ip in 192.168.0.0/16",
            input(vec![(
                "destination",
                object(vec![("ip", ip("192.168.2.37"))]),
            )]),
        ),
        case(
            "missing_fields",
            r#"user == "root" or destination.ip in 192.168.0.0/16"#,
            input(vec![]),
        ),
        case(
            "function",
            r#"starts_with(path, "/api")"#,
            input(vec![("path", s("/api/v1"))]),
        ),
        macro_case,
        custom,
        case(
            "url_field",
            r#"u.host == "example.com" and u.port == 8443 and u =~ /^https:/"#,
            input(vec![(
                "u",
                Value::Url(Box::new(
                    Url::parse("https://Example.com:8443/a?b=c").unwrap(),
                )),
            )]),
        ),
    ]
}

/// Go `BenchmarkEvalTrace`.
pub fn trace_case() -> EvalCase {
    let mut c = case("BenchmarkEvalTrace", FULL, traversal("192.168.2.37"));
    c.trace = true;
    c
}

/// An input whose `expensive` field is a lazy value.
pub fn lazy_input() -> KvInput {
    let lazy = KvEntry::Lazy(Lazy::new(|_: &()| Ok(s("value"))));
    KvInput::new(
        [
            ("allow".to_owned(), KvEntry::Value(Value::Bool(true))),
            ("expensive".to_owned(), lazy),
        ]
        .into_iter()
        .collect(),
    )
}

/// Go `BenchmarkEvalLazyInput`: (name, rule, warm the input first).
pub fn lazy_cases() -> Vec<(&'static str, Rule, bool)> {
    vec![
        (
            "pruned",
            rulekit::parse(r#"allow == true or expensive == "value""#).unwrap(),
            false,
        ),
        (
            "resolved_cached",
            rulekit::parse(r#"expensive == "value""#).unwrap(),
            true,
        ),
    ]
}

/// Go `BenchmarkParse`.
pub const PARSE_CASES: [(&str, &str); 2] = [
    ("simple", "tags eq 'db-svc'"),
    (
        "complex",
        r"tags eq 'db-svc' OR domain matches /example\.com$/ OR (process.uid != 0 AND tags contains 'internal-svc')",
    ),
];

/// Go `BenchmarkCmpNumber` operand types: (Go type name, value). Go also has
/// int, uint, and float32; Rust numbers are i64, u64, and f64.
pub fn cmp_values() -> [(&'static str, ValueRef<'static>); 4] {
    [
        ("int64", ValueRef::Int(1)),
        ("uint64", ValueRef::Uint(1)),
        ("float64", ValueRef::Float(1.0)),
        ("string", ValueRef::Str("1")),
    ]
}

#[derive(rulekit::Args)]
struct PrefixArgs<'a> {
    value: &'a str,
    prefix: &'a str,
}

/// A custom function with borrowed `&str` arguments.
fn has_prefix() -> Function {
    Function::new::<PrefixArgs, bool>(
        FuncSchema {
            name: "has_prefix",
            doc: "",
        },
        |_: &(), a| Ok(a.value.starts_with(a.prefix)),
    )
}

#[derive(rulekit::Input)]
pub struct DerivedInput {
    pub host: String,
    pub port: u16,
    pub tags: Vec<String>,
    pub headers: HashMap<String, String>,
}

pub struct DerivedBench {
    pub rule: Rule,
    pub input: DerivedInput,
    pub kv: EvalCase,
}

impl DerivedBench {
    pub fn new() -> Self {
        let expr = r#"host == "api.acme.com" and port == 8443 and tags contains "db" and headers["x-env"] == "prod""#;
        let kv = case(
            "derived_vs_kv",
            expr,
            input(vec![
                ("host", s("api.acme.com")),
                ("port", Value::Int(8443)),
                ("tags", Value::Array(vec![s("db"), s("api")])),
                ("headers", object(vec![("x-env", s("prod"))])),
            ]),
        );
        Self {
            rule: rulekit::parse(expr).expect("parse"),
            input: DerivedInput {
                host: "api.acme.com".into(),
                port: 8443,
                tags: vec!["db".into(), "api".into()],
                headers: HashMap::from([("x-env".into(), "prod".into())]),
            },
            kv,
        }
    }

    pub fn eval(&self) -> bool {
        self.rule.eval(&(), &self.input, Opts::default()).pass()
    }
}
