//! Happy-path evaluation must not allocate. Mirrors the cases of Go's
//! `BenchmarkEval`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use rulekit::value::{Map, Value};
use rulekit::{Env, KvInput, Opts};

#[path = "../benches/cases.rs"]
mod cases;

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

#[test]
fn happy_path_eval_does_not_allocate() {
    let no_macros = Env::<()>::new();
    let mut eval_cases = cases::eval_cases();

    // Allocation-only coverage beyond the shared benchmark cases.
    eval_cases.extend(
        [
            (
                "function_list_from_input",
                "starts_with(type, prefixes)",
                input(vec![
                    ("type", s("installation.complete")),
                    (
                        "prefixes",
                        Value::Array(vec![s("process."), s("agent."), s("installation.")]),
                    ),
                ]),
            ),
            (
                "function_list_text_form_items",
                "starts_with(addr, [192.168.1.1, 10.0.0.1])",
                input(vec![("addr", s("10.0.0.1:443"))]),
            ),
            (
                "array_literal_with_field",
                "port in [80, other_port, 443]",
                input(vec![
                    ("port", Value::Int(443)),
                    ("other_port", Value::Int(8080)),
                ]),
            ),
        ]
        .into_iter()
        .map(|(name, expr, input)| cases::EvalCase {
            name,
            rule: rulekit::parse(expr).expect("parse"),
            input,
            env: Env::new(),
            trace: false,
        }),
    );

    eval_cases.extend(
        cases::lazy_cases()
            .into_iter()
            .map(|(name, rule, _)| cases::EvalCase {
                name,
                rule,
                input: cases::lazy_input(),
                env: Env::new(),
                trace: false,
            }),
    );

    let mut failures = Vec::new();
    for case in &eval_cases {
        let cases::EvalCase {
            name,
            rule,
            input,
            env,
            ..
        } = case;
        // Warm up: lazily built statics and resolved lazy inputs may allocate once.
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

#[test]
fn derived_struct_eval_does_not_allocate() {
    use std::collections::HashMap;

    #[derive(rulekit::Input)]
    struct User<'a> {
        name: &'a str,
    }
    #[derive(rulekit::Input)]
    struct Row<'a> {
        host: &'a str,
        user: User<'a>,
        tags: Vec<String>,
        headers: HashMap<String, String>,
    }
    let row = Row {
        host: "api.acme.com",
        user: User { name: "ada" },
        tags: vec!["db".into(), "api".into()],
        headers: HashMap::from([("x-env".into(), "prod".into())]),
    };
    let rule = rulekit::parse(
        r#"host == "api.acme.com" and user.name == "ada" and tags contains "db" and headers["x-env"] == "prod""#,
    )
    .unwrap();
    drop(rule.eval(&(), &row, Opts::default()));
    let before = allocations();
    assert!(rule.eval(&(), &row, Opts::default()).pass());
    assert_eq!(allocations(), before);
}

#[test]
fn serde_shaped_event_eval_does_not_allocate() {
    #[derive(rulekit::Input)]
    #[serde(untagged)]
    enum EventRecord {
        Entity(EntityRecord),
    }
    #[derive(rulekit::Input)]
    struct EntityRecord {
        severity: Severity,
        entity_id: String,
        #[serde(flatten)]
        event: EntityEvent,
    }
    #[derive(rulekit::Input)]
    #[serde(rename_all = "lowercase")]
    enum Severity {
        Info,
        Warn,
    }
    #[derive(rulekit::Input)]
    #[serde(untagged)]
    enum EntityEvent {
        Process(ProcessEvent),
        Llm(LlmEvent),
    }
    #[derive(rulekit::Input)]
    #[serde(tag = "type", content = "payload")]
    enum ProcessEvent {
        #[serde(rename = "process.started")]
        Started { pid: u32 },
    }
    #[derive(rulekit::Input)]
    #[serde(tag = "type", content = "payload")]
    enum LlmEvent {
        #[serde(rename = "llm.request")]
        Request(Request),
    }
    #[derive(rulekit::Input)]
    struct Request {
        model: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        system: Option<String>,
    }
    let llm = EventRecord::Entity(EntityRecord {
        severity: Severity::Warn,
        entity_id: "e-1".into(),
        event: EntityEvent::Llm(LlmEvent::Request(Request {
            model: "gpt-x".into(),
            request_id: None,
            system: None,
        })),
    });
    let process = EventRecord::Entity(EntityRecord {
        severity: Severity::Info,
        entity_id: "e-2".into(),
        event: EntityEvent::Process(ProcessEvent::Started { pid: 7 }),
    });
    for (record, expr) in [
        (&llm, r#"type == "llm.request""#),
        (&llm, r#"payload.model == "gpt-x""#),
        (&llm, r#"severity == "warn""#),
        (
            &llm,
            r#"starts_with(type, "process.") or payload.model == "gpt-x""#,
        ),
        (
            &llm,
            r#"entity_id == "e-1" and not starts_with(payload.model, "claude")"#,
        ),
        (
            &process,
            r#"type == "process.started" and payload.pid == 7 and severity == "info""#,
        ),
    ] {
        let rule = rulekit::parse(expr).unwrap();
        drop(rule.eval(&(), record, Opts::default()));
        let before = allocations();
        assert!(rule.eval(&(), record, Opts::default()).pass(), "{expr}");
        assert_eq!(allocations(), before, "{expr}");
    }
}

#[test]
fn bytes_field_does_not_allocate() {
    #[derive(rulekit::Input)]
    struct Row<'a> {
        #[rulekit(bytes)]
        body: &'a [u8],
        #[rulekit(bytes)]
        owned: Vec<u8>,
        raw: bytes::Bytes,
    }
    let row = Row {
        body: b"POST",
        owned: b"POST".to_vec(),
        raw: bytes::Bytes::from_static(b"POST"),
    };
    let rule =
        rulekit::parse(r#"body == "POST" and owned contains "OS" and raw == "POST""#).unwrap();
    drop(rule.eval(&(), &row, Opts::default()));
    let before = allocations();
    assert!(rule.eval(&(), &row, Opts::default()).pass());
    assert_eq!(allocations(), before);
}

#[test]
fn derived_url_field_access_does_not_allocate() {
    #[derive(rulekit::Input)]
    struct Row {
        page: url::Url,
        host: &'static str,
    }
    let row = Row {
        page: url::Url::parse("https://example.com/a?env=prod").unwrap(),
        host: "example.com",
    };
    let rule = rulekit::parse(
        r#"host == "example.com" and page.host == "example.com" and page.path == "/a" and page.query.env == "prod""#,
    )
    .unwrap();
    // Warm any one-time init, then count.
    drop(rule.eval(&(), &row, Opts::default()));
    let before = allocations();
    assert!(rule.eval(&(), &row, Opts::default()).pass());
    assert_eq!(allocations(), before);
}

#[test]
fn http_uri_field_access_does_not_allocate() {
    #[derive(rulekit::Input)]
    struct Row {
        page: http::Uri,
    }
    let row = Row {
        page: "http://example.com/a?env=prod".parse().unwrap(),
    };
    let fields = rulekit::parse(
        r#"page.host == "example.com" and page.path == "/a" and page.query.env == "prod""#,
    )
    .unwrap();
    drop(fields.eval(&(), &row, Opts::default()));
    let before = allocations();
    assert!(fields.eval(&(), &row, Opts::default()).pass());
    assert_eq!(allocations(), before, "field access allocated");

    let whole = rulekit::parse(r#"page == "http://example.com/a?env=prod""#).unwrap();
    drop(whole.eval(&(), &row, Opts::default()));
    let before = allocations();
    assert!(whole.eval(&(), &row, Opts::default()).pass());
    let used = allocations() - before;
    // Text form is borrowed parts; comparison must not allocate.
    assert_eq!(used, 0, "http::Uri comparison allocated");
}

#[test]
fn unread_lazy_does_not_allocate_and_second_read_is_free() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    let input = rulekit::kv! {
        "host" => "api.acme.com",
        "user" => rulekit::lazy(move |_: &()| {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok::<_, rulekit::BoxError>("ada")
        }),
    };
    let unread = rulekit::parse(r#"host == "api.acme.com""#).unwrap();
    drop(unread.eval(&(), &input, Opts::default()));
    let before = allocations();
    assert!(unread.eval(&(), &input, Opts::default()).pass());
    assert_eq!(allocations(), before);
    assert_eq!(hits.load(Ordering::SeqCst), 0);

    let read = rulekit::parse(r#"user == "ada""#).unwrap();
    assert!(read.eval(&(), &input, Opts::default()).pass());
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    let before = allocations();
    assert!(read.eval(&(), &input, Opts::default()).pass());
    assert_eq!(allocations(), before, "memoized lazy read allocated");
    assert_eq!(hits.load(Ordering::SeqCst), 1);
}
