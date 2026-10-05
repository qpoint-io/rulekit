//! `kv!` and `lazy`.

use std::sync::atomic::{AtomicUsize, Ordering};

use rulekit::{LazyVal, Opts, kv, lazy};

#[test]
fn kv_nested_and_lazy_is_unread() {
    let hits = AtomicUsize::new(0);
    let input = kv! {
        "host" => "api.acme.com",
        "port" => 8443u16,
        "user" => { "id" => 42u64 },
        "boom" => lazy(|_: &()| {
            hits.fetch_add(1, Ordering::SeqCst);
            Ok(rulekit::value::Val::from("nope"))
        }),
    };
    let rule =
        rulekit::parse(r#"host == "api.acme.com" and port == 8443 and user.id == 42"#).unwrap();
    assert!(rule.eval(&(), &input, Opts::default()).pass());
    assert_eq!(hits.load(Ordering::SeqCst), 0);

    let rule = rulekit::parse(r#"boom == "nope""#).unwrap();
    assert!(rule.eval(&(), &input, Opts::default()).pass());
    assert_eq!(hits.load(Ordering::SeqCst), 1);
}

#[test]
fn lazy_field_on_a_derived_struct() {
    #[derive(rulekit::Input)]
    struct Row {
        host: &'static str,
        user: LazyVal<'static, ()>,
    }
    let row = Row {
        host: "api.acme.com",
        user: lazy(|_: &()| panic!("unread")),
    };
    let rule = rulekit::parse(r#"host == "api.acme.com""#).unwrap();
    assert!(rule.eval(&(), &row, Opts::default()).pass());
}
