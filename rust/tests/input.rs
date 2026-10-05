//! Inputs and lazy values (ports the intent of Go `input_test.go`).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

use rulekit::ast::Segment;
use rulekit::value::Value;
use rulekit::{FnInput, Kv, KvEntry, KvInput, Lazy, Opts};

fn s(v: &str) -> Value {
    Value::String(v.to_owned())
}

fn kv<C: ?Sized>(entries: Vec<(&str, KvEntry<C>)>) -> Kv<C> {
    entries
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v))
        .collect()
}

#[test]
fn lazy_value_resolves_once() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let input = KvInput::new(kv(vec![(
        "expensive",
        KvEntry::Lazy(Lazy::new(move |_: &()| {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(s("value"))
        })),
    )]));
    let rule = rulekit::parse(r#"expensive == "value" and expensive == "value""#).unwrap();
    let result = rule.eval(&input, &(), Opts::default());
    assert!(result.error_ref().is_none());
    assert!(result.pass());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn lazy_memo_keeps_bracket_keys_distinct() {
    let lazy = |v: &'static str| KvEntry::Lazy(Lazy::new(move |_: &()| Ok(s(v))));
    let input = KvInput::new(kv(vec![
        ("a.b", lazy("flat")),
        ("a", KvEntry::Object(kv(vec![("b", lazy("nested"))]))),
    ]));
    let rule =
        rulekit::parse(r#"["a.b"] == "flat" and a.b == "nested" and a["b"] == "nested""#).unwrap();
    let result = rule.eval(&input, &(), Opts::default());
    assert!(result.error_ref().is_none());
    assert!(result.pass());
}

struct Ctx {
    user: String,
}

#[test]
fn lazy_value_reads_the_context() {
    let input = KvInput::new(kv(vec![(
        "user",
        KvEntry::Lazy(Lazy::new(|ctx: &Ctx| Ok(s(&ctx.user)))),
    )]));
    let rule = rulekit::parse(r#"user == "root""#).unwrap();
    let env = rulekit::Env::new();
    let ctx = Ctx {
        user: "root".into(),
    };
    let result = rule.eval(&input, &ctx, Opts::new(&env));
    assert!(result.pass());
}

#[test]
fn nested_input_takes_over_the_subtree() {
    let request = FnInput(|_: &(), path: &[Segment]| {
        assert_eq!(
            path,
            [
                Segment::Key {
                    key: "headers".into(),
                    bracket: false
                },
                Segment::Key {
                    key: "user-agent".into(),
                    bracket: true
                }
            ]
        );
        Ok(Some(s("curl")))
    });
    let input = KvInput::new(kv(vec![("request", KvEntry::Input(Arc::new(request)))]));
    let rule = rulekit::parse(r#"request.headers["user-agent"] == "curl""#).unwrap();
    let result = rule.eval(&input, &(), Opts::default());
    assert!(result.error_ref().is_none());
    assert!(result.pass());
}

/// One input shared by concurrent evaluations: each lazy resolves exactly once.
#[test]
fn concurrent_eval_resolves_each_lazy_once() {
    let (plain, user, nested) = (
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AtomicUsize::new(0)),
    );
    let counted = |calls: &Arc<AtomicUsize>, f: fn(&Ctx) -> Value| {
        let calls = calls.clone();
        KvEntry::Lazy(Lazy::new(move |ctx: &Ctx| {
            calls.fetch_add(1, Ordering::SeqCst);
            thread::sleep(Duration::from_millis(1));
            Ok(f(ctx))
        }))
    };
    let input = KvInput::new(kv(vec![
        ("static", KvEntry::Value(s("value"))),
        ("plain", counted(&plain, |_| s("plain"))),
        ("user", counted(&user, |ctx| s(&ctx.user))),
        (
            "request",
            KvEntry::Object(kv(vec![("id", counted(&nested, |_| s("req-1")))])),
        ),
    ]));
    let rules: Vec<_> = [
        r#"static == "value""#,
        r#"plain == "plain""#,
        r#"user == "root""#,
        r#"request.id == "req-1""#,
        r#"plain == "plain" and user == "root" and request.id == "req-1" and static == "value""#,
    ]
    .iter()
    .map(|expr| rulekit::parse(expr).unwrap())
    .collect();
    let ctx = Ctx {
        user: "root".into(),
    };
    let env = rulekit::Env::new();

    thread::scope(|scope| {
        for g in 0..16 {
            let (rules, input, ctx, env) = (&rules, &input, &ctx, &env);
            scope.spawn(move || {
                for i in 0..50 {
                    let rule = &rules[(g + i) % rules.len()];
                    let result = rule.eval(input, ctx, Opts::new(env));
                    assert!(result.error_ref().is_none() && result.pass(), "{rule}");
                }
            });
        }
    });

    assert_eq!(plain.load(Ordering::SeqCst), 1);
    assert_eq!(user.load(Ordering::SeqCst), 1);
    assert_eq!(nested.load(Ordering::SeqCst), 1);
}

#[test]
fn lazy_errors_are_not_memoized() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let input = KvInput::new(kv(vec![(
        "flaky",
        KvEntry::Lazy(Lazy::new(move |_: &()| {
            if counter.fetch_add(1, Ordering::SeqCst) == 0 {
                return Err("boom".into());
            }
            Ok(s("ok"))
        })),
    )]));
    let rule = rulekit::parse(r#"flaky == "ok""#).unwrap();
    assert!(
        rule.eval(&input, &(), Opts::default())
            .error_ref()
            .is_some()
    );
    let result = rule.eval(&input, &(), Opts::default());
    assert!(result.error_ref().is_none());
    assert!(result.pass());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn cloned_lazy_is_unresolved() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let tree = kv(vec![(
        "x",
        KvEntry::Lazy(Lazy::new(move |_: &()| {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(Value::Int(1))
        })),
    )]);
    let rule = rulekit::parse("x == 1").unwrap();
    let first = KvInput::new(tree.clone());
    assert!(rule.eval(&first, &(), Opts::default()).pass());
    let second = KvInput::new(tree);
    assert!(rule.eval(&second, &(), Opts::default()).pass());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}
