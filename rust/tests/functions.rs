//! Custom functions, macros, and environment validation.

use rulekit::value::{Map, Val, Value, ValueRef};
use rulekit::{ArgSpec, Env, Error, Function, KvInput, NoInput, Opts, Type};

fn input(entries: &[(&str, Value)]) -> KvInput {
    KvInput::from_values(
        entries
            .iter()
            .cloned()
            .map(|(k, v)| (k.to_owned(), v))
            .collect::<Map<Value>>(),
    )
}

#[test]
fn custom_function_reads_args_by_index_and_name() {
    let add = Function::new(
        [ArgSpec::new("a"), ArgSpec::typed("b", Type::Int64)],
        |_: &(), args| {
            let a: i64 = args.index(0)?;
            let b: i64 = args.by_name("b")?;
            Ok(Val::Owned(Value::Int(a + b)))
        },
    );
    let env = Env::builder().function("add", add).build().unwrap();
    let rule = rulekit::parse("add(x, 2) == 5").unwrap();
    let kv = input(&[("x", Value::Int(3))]);
    assert!(rule.eval(&(), &kv, Opts::new(&env)).pass());

    // A typed argument is checked before the function runs.
    let rule = rulekit::parse(r#"add(1, "2") == 3"#).unwrap();
    let result = rule.eval(&(), &NoInput, Opts::new(&env));
    assert!(matches!(result.error(), Some(Error::InvalidArg { name, .. }) if name == "b"));

    // Wrong arity.
    let rule = rulekit::parse("add(1) == 1").unwrap();
    let result = rule.eval(&(), &NoInput, Opts::new(&env));
    assert!(matches!(
        result.error(),
        Some(Error::ArgCount {
            expected: 2,
            got: 1,
            ..
        })
    ));

    // Missing arguments make the call unknown, not an error.
    let rule = rulekit::parse("add(y, 1) == 1").unwrap();
    let result = rule.eval(&(), &NoInput, Opts::new(&env));
    assert!(result.unknown());
    assert_eq!(result.missing_fields().collect::<Vec<_>>(), ["y"]);
}

#[test]
fn functions_receive_the_context() {
    struct Ctx {
        tenant: String,
    }
    let tenant = Function::new([], |ctx: &Ctx, _| Ok(Val::Ref(ValueRef::Str(&ctx.tenant))));
    let env = Env::builder().function("tenant", tenant).build().unwrap();
    let rule = rulekit::parse(r#"tenant() == "acme""#).unwrap();
    let ctx = Ctx {
        tenant: "acme".into(),
    };
    assert!(rule.eval(&ctx, &NoInput, Opts::new(&env)).pass());
}

#[test]
fn env_validation() {
    let noop = || Function::<()>::new([], |_, _| Ok(Val::Ref(ValueRef::Bool(true))));
    assert!(
        Env::builder()
            .function("starts_with", noop())
            .build()
            .is_err()
    );
    assert!(
        Env::<()>::builder()
            .macro_source("starts_with", "true")
            .unwrap()
            .build()
            .is_err()
    );
    assert!(
        Env::builder()
            .function("m", noop())
            .macro_source("m", "true")
            .unwrap()
            .build()
            .is_err()
    );
    assert!(Env::<()>::builder().macro_source("bad", "a ==").is_err());
}

#[test]
fn macros_expand_and_reject_arguments() {
    let env = Env::<()>::builder()
        .macro_source("internal", "ip in 10.0.0.0/8")
        .unwrap()
        .build()
        .unwrap();
    let kv = input(&[(
        "ip",
        Value::Ip(rulekit::value::Ip::parse("10.1.2.3").unwrap()),
    )]);
    assert!(
        rulekit::parse("internal()")
            .unwrap()
            .eval(&(), &kv, Opts::new(&env))
            .pass()
    );
    let rule = rulekit::parse("internal(1)").unwrap();
    let result = rule.eval(&(), &kv, Opts::new(&env));
    assert!(matches!(result.error(), Some(Error::MacroArgs { .. })));
    let rule = rulekit::parse("nope()").unwrap();
    let result = rule.eval(&(), &kv, Opts::default());
    assert!(matches!(result.error(), Some(Error::UnknownFunction(name)) if name == "nope"));
}

#[test]
fn functions_can_report_missing_fields_and_errors() {
    use rulekit::FnError;
    let lookup = Function::new([ArgSpec::new("key")], |_: &(), args| {
        match args.index::<&str>(0)? {
            "known" => Ok(Val::from("value")),
            "absent" => Err(FnError::missing(["geo.country"])),
            _ => Err(FnError::msg("unsupported key")),
        }
    });
    let env = Env::builder().function("lookup", lookup).build().unwrap();
    let eval = |expr: &str| {
        let rule = rulekit::parse(expr).unwrap();
        let result = rule.eval(&(), &NoInput, Opts::new(&env));
        (
            result.pass(),
            result
                .missing_fields()
                .map(|s| s.to_string())
                .collect::<Vec<_>>(),
            result.error().is_some(),
        )
    };
    assert_eq!(eval(r#"lookup("known") == "value""#), (true, vec![], false));
    assert_eq!(
        eval(r#"lookup("absent") == "x""#),
        (false, vec!["geo.country".to_owned()], false)
    );
    assert_eq!(eval(r#"lookup("other") == "x""#), (false, vec![], true));
}

#[test]
fn macros_and_functions_carry_docs() {
    let m = rulekit::Macro::new("a == 1").unwrap().with_doc("A is one.");
    assert_eq!(m.doc(), Some("A is one."));
    let f = Function::<()>::new([], |_, _| Ok(Val::from("x"))).with_doc("Returns x.");
    assert_eq!(f.doc(), Some("Returns x."));
}
