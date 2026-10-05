//! `kv!` and `lazy`.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use rulekit::value::Value;
use rulekit::{BoxError, Lazy, Opts, kv, lazy};

#[test]
fn kv_nested_and_lazy_is_unread() {
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    let input = kv! {
        "host" => "api.acme.com",
        "port" => 8443u16,
        "user" => { "id" => 42u64 },
        "boom" => lazy(move |_: &()| {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok::<_, BoxError>("nope")
        }),
    };
    let rule =
        rulekit::parse(r#"host == "api.acme.com" and port == 8443 and user.id == 42"#).unwrap();
    assert!(rule.eval(&(), &input, Opts::default()).pass());
    assert_eq!(hits.load(Ordering::SeqCst), 0);
}

#[test]
fn lazy_is_computed_once_across_evals() {
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    let input = kv! {
        "boom" => lazy(move |_: &()| {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok::<_, BoxError>("nope")
        }),
    };
    let rule = rulekit::parse(r#"boom == "nope" and boom == "nope""#).unwrap();
    assert!(rule.eval(&(), &input, Opts::default()).pass());
    assert!(rule.eval(&(), &input, Opts::default()).pass());
    assert_eq!(hits.load(Ordering::SeqCst), 1);
}

#[test]
fn lazy_is_single_flight_across_threads() {
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    let input = Arc::new(kv! {
        "boom" => lazy(move |_: &()| {
            counter.fetch_add(1, Ordering::SeqCst);
            thread::sleep(std::time::Duration::from_millis(20));
            Ok::<_, BoxError>("nope")
        }),
    });
    let rule = Arc::new(rulekit::parse(r#"boom == "nope""#).unwrap());
    let mut threads = Vec::new();
    for _ in 0..8 {
        let input = input.clone();
        let rule = rule.clone();
        threads.push(thread::spawn(move || {
            assert!(rule.eval(&(), input.as_ref(), Opts::default()).pass());
        }));
    }
    for thread in threads {
        thread.join().unwrap();
    }
    assert_eq!(hits.load(Ordering::SeqCst), 1);
}

#[test]
fn lazy_error_is_not_memoized() {
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    let input = kv! {
        "flaky" => lazy(move |_: &()| {
            let n = counter.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                Err(BoxError::from("boom"))
            } else {
                Ok("ok")
            }
        }),
    };
    let rule = rulekit::parse(r#"flaky == "ok""#).unwrap();
    assert!(rule.eval(&(), &input, Opts::default()).error().is_some());
    assert!(rule.eval(&(), &input, Opts::default()).pass());
    assert_eq!(hits.load(Ordering::SeqCst), 2);
}

#[test]
fn lazy_field_on_a_derived_struct() {
    #[derive(rulekit::Input)]
    struct Row {
        host: &'static str,
        user: Lazy,
    }
    let row = Row {
        host: "api.acme.com",
        user: lazy(|_: &()| -> Result<&'static str, BoxError> { panic!("unread") }),
    };
    let rule = rulekit::parse(r#"host == "api.acme.com""#).unwrap();
    assert!(rule.eval(&(), &row, Opts::default()).pass());
}

#[test]
fn lazy_returns_owned_value() {
    struct Ctx {
        user: String,
    }
    let input = kv! {
        "user" => lazy(|ctx: &Ctx| Ok(ctx.user.clone())),
    };
    let rule = rulekit::parse(r#"user == "ada""#).unwrap();
    let env: rulekit::Env<Ctx> = rulekit::Env::new();
    let ctx = Ctx { user: "ada".into() };
    assert!(rule.eval(&ctx, &input, Opts::new(&env)).pass());
    let _ = Value::Null;
}

#[test]
fn kv_bytes_wrapper_is_not_a_list() {
    let buf = b"POST";
    let input = kv! {
        "body" => rulekit::bytes(buf),
        "nums" => buf.to_vec(),
        "raw" => bytes::Bytes::from_static(buf),
    };
    let rule = rulekit::parse(r#"body == "POST" and raw == "POST" and nums[0] == 80"#).unwrap();
    assert!(rule.eval(&(), &input, Opts::default()).pass());
    let as_text = rulekit::parse(r#"nums == "POST""#).unwrap();
    assert!(!as_text.eval(&(), &input, Opts::default()).pass());
}
