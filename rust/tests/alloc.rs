//! Happy-path evaluation must not allocate. Mirrors the cases of Go's
//! `BenchmarkEval`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use rulekit::value::{Ip, Map, Value};
use rulekit::{Env, KvEntry, KvInput, Lazy, Opts};

struct Counting;

thread_local! {
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = ALLOCATIONS.try_with(|n| n.set(n.get() + 1));
        // SAFETY: forwarded unchanged to the system allocator.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: forwarded unchanged to the system allocator.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn allocations() -> usize {
    ALLOCATIONS.with(Cell::get)
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

fn s(v: &str) -> Value {
    Value::String(v.to_owned())
}

fn ip(v: &str) -> Value {
    Value::Ip(Ip::parse(v).expect("ip"))
}

#[test]
fn happy_path_eval_does_not_allocate() {
    let first =
        r#"tags == "db-svc" or domain matches /example\.com$/ or destination.ip in 192.168.0.0/16"#;
    let full = r#"tags == "db-svc" or domain matches /example\.com$/ or process.uid == 0 or destination.ip in 192.168.0.0/16"#;
    let traversal = |dst: &str| {
        input(vec![
            ("tags", s("other")),
            ("domain", s("qpoint.io")),
            ("process", object(vec![("uid", Value::Int(1000))])),
            ("destination", object(vec![("ip", ip(dst))])),
        ])
    };
    let macros = Env::<()>::builder()
        .macro_source("is_internal", "ip in 172.16.0.0/16")
        .unwrap()
        .build()
        .unwrap();
    let no_macros = Env::<()>::new();

    let cases: Vec<(&str, &str, KvInput, &Env)> = vec![
        (
            "short_circuit_first_branch_string",
            first,
            input(vec![("tags", s("db-svc"))]),
            &no_macros,
        ),
        (
            "short_circuit_first_branch_string_slice",
            first,
            input(vec![(
                "tags",
                Value::Array(vec![s("db-svc"), s("internal-vlan")]),
            )]),
            &no_macros,
        ),
        (
            "full_traversal_last_branch_pass",
            full,
            traversal("192.168.2.37"),
            &no_macros,
        ),
        (
            "full_traversal_no_match",
            full,
            traversal("10.0.0.1"),
            &no_macros,
        ),
        (
            "nested_path_number",
            "process.uid != 0 and destination.port <= 1023",
            input(vec![
                ("process", object(vec![("uid", Value::Int(1000))])),
                ("destination", object(vec![("port", Value::Int(443))])),
            ]),
            &no_macros,
        ),
        (
            "bracket_path",
            r#"request.headers["user-agent"] == "curl""#,
            input(vec![(
                "request",
                object(vec![("headers", object(vec![("user-agent", s("curl"))]))]),
            )]),
            &no_macros,
        ),
        (
            "array_index_path",
            r#"items[0].name == "first""#,
            input(vec![(
                "items",
                Value::Array(vec![
                    object(vec![("name", s("first"))]),
                    object(vec![("name", s("second"))]),
                ]),
            )]),
            &no_macros,
        ),
        (
            "regex",
            r"domain matches /example\.com$/",
            input(vec![("domain", s("api.example.com"))]),
            &no_macros,
        ),
        (
            "ip_cidr",
            "destination.ip in 192.168.0.0/16",
            input(vec![(
                "destination",
                object(vec![("ip", ip("192.168.2.37"))]),
            )]),
            &no_macros,
        ),
        (
            "missing_fields",
            r#"user == "root" or destination.ip in 192.168.0.0/16"#,
            input(vec![]),
            &no_macros,
        ),
        (
            "function",
            r#"starts_with(path, "/api")"#,
            input(vec![("path", s("/api/v1"))]),
            &no_macros,
        ),
        (
            "macro",
            r#"is_internal() and user != "root""#,
            input(vec![("ip", ip("172.16.0.1")), ("user", s("api"))]),
            &macros,
        ),
        (
            "array_literal_with_field",
            "port in [80, other_port, 443]",
            input(vec![
                ("port", Value::Int(443)),
                ("other_port", Value::Int(8080)),
            ]),
            &no_macros,
        ),
    ];

    // Go BenchmarkEvalLazyInput: a pruned lazy, and a resolved (cached) lazy.
    let lazy_input = || {
        let lazy = KvEntry::Lazy(Lazy::new(|_: &()| Ok(Value::String("value".into()))));
        KvInput::new(
            [
                ("allow".to_owned(), KvEntry::Value(Value::Bool(true))),
                ("expensive".to_owned(), lazy),
            ]
            .into_iter()
            .collect(),
        )
    };
    let mut cases = cases;
    cases.push((
        "lazy_pruned",
        r#"allow == true or expensive == "value""#,
        lazy_input(),
        &no_macros,
    ));
    cases.push((
        "lazy_resolved_cached",
        r#"expensive == "value""#,
        lazy_input(),
        &no_macros,
    ));

    let mut failures = Vec::new();
    for (name, expr, input, env) in &cases {
        let rule = rulekit::parse(expr).expect("parse");
        // Warm up: lazily built statics (e.g. regex caches) may allocate once.
        drop(rule.eval(&(), input, Opts::new(env)));
        let before = allocations();
        let result = rule.eval(&(), input, Opts::new(env).with_trace(false));
        let used = allocations() - before;
        assert!(result.error().is_none(), "{name}: {:?}", result.error());
        drop(result);
        if used != 0 {
            failures.push(format!("{name}: {used} allocations"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));

    // Control: an array of fields as the rule's value is materialized (as in
    // Go), so the counter must see it.
    let rule = rulekit::parse("[port]").expect("parse");
    let control = input(vec![("port", Value::Int(1))]);
    let before = allocations();
    drop(rule.eval(&(), &control, Opts::new(&no_macros)));
    assert!(allocations() > before, "allocation counter is not counting");
}
