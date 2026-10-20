//! Custom functions, macros, and options validation.

use rulekit::value::{Map, Value, ValueRef};
use rulekit::{Error, FnError, FuncSchema, Function, KvInput, NoArgs, NoInput, Opts, Rest};

fn input(entries: &[(&str, Value)]) -> KvInput {
    KvInput::from_values(
        entries
            .iter()
            .cloned()
            .map(|(k, v)| (k.to_owned(), v))
            .collect::<Map<Value>>(),
    )
}

#[derive(rulekit::Args)]
struct AddArgs {
    a: i64,
    #[rulekit(rename = "b")]
    second: i64,
}

#[derive(rulekit::Args)]
struct KeyArgs<'a> {
    key: &'a str,
}

#[derive(rulekit::Args)]
struct CountArgs<'a> {
    list: rulekit::value::ArrayRef<'a>,
}

#[derive(rulekit::Args)]
struct JoinArgs<'a> {
    separator: &'a str,
    parts: Rest<'a>,
}

#[test]
fn typed_arguments_and_return() {
    let add = Function::new::<AddArgs, i64>(
        FuncSchema {
            name: "add",
            doc: "a + b",
        },
        |args| Ok(args.a + args.second),
    );
    assert_eq!(add.name(), "add");
    assert_eq!(add.doc(), "a + b");
    assert_eq!(add.returns(), "int64");
    let params: Vec<_> = add
        .params()
        .iter()
        .map(|p| (p.name(), p.ty(), p.is_rest()))
        .collect();
    assert_eq!(params, [("a", "int64", false), ("b", "int64", false)]);

    let opts = Opts::builder().function(add).build().unwrap();
    let rule = rulekit::parse("add(x, 2) == 5").unwrap();
    let kv = input(&[("x", Value::Int(3))]);
    assert!(rule.eval(&kv, &opts).pass());

    // A wrongly typed argument is an error naming the parameter.
    let rule = rulekit::parse(r#"add(1, "2") == 3"#).unwrap();
    let result = rule.eval(&NoInput, &opts);
    assert!(
        matches!(result.error(), Some(Error::InvalidArg { name, expected, got })
        if name == "b" && expected == "int64" && got == "string")
    );

    // Wrong arity.
    let rule = rulekit::parse("add(1) == 1").unwrap();
    let result = rule.eval(&NoInput, &opts);
    assert!(matches!(
        result.error(),
        Some(Error::ArgCount {
            expected: 2,
            got: 1,
            variadic: false,
            ..
        })
    ));

    // Missing arguments make the call unknown, not an error.
    let rule = rulekit::parse("add(y, 1) == 1").unwrap();
    let result = rule.eval(&NoInput, &opts);
    assert!(result.unknown());
    assert_eq!(result.missing_fields().collect::<Vec<_>>(), ["y"]);
}

#[test]
fn borrowed_arguments_and_rest() {
    let join = Function::new::<JoinArgs, String>(
        FuncSchema {
            name: "join",
            doc: "",
        },
        |a| {
            let parts: Vec<String> = a
                .parts
                .iter()
                .map(|p| p.text().map_or_else(String::new, |t| t.to_string()))
                .collect();
            Ok(parts.join(a.separator))
        },
    );
    assert_eq!(
        join.params().last().map(|p| (p.name(), p.is_rest())),
        Some(("parts", true))
    );
    let opts = Opts::builder().function(join).build().unwrap();
    let kv = input(&[("x", Value::String("b".into()))]);
    let rule = rulekit::parse(r#"join("-", "a", x, "c") == "a-b-c" and join(",") == """#).unwrap();
    assert!(rule.eval(&kv, &opts).pass());

    let rule = rulekit::parse("join() == 1").unwrap();
    let result = rule.eval(&kv, &opts);
    assert!(matches!(
        result.error(),
        Some(Error::ArgCount {
            expected: 1,
            got: 0,
            variadic: true,
            ..
        })
    ));
}

#[test]
fn functions_capture_state() {
    let tenant_name = String::from("acme");
    let tenant = Function::new::<NoArgs, String>(
        FuncSchema {
            name: "tenant",
            doc: "",
        },
        move |_| Ok(tenant_name.clone()),
    );
    assert_eq!(tenant.returns(), "string");
    let opts = Opts::builder().function(tenant).build().unwrap();
    let rule = rulekit::parse(r#"tenant() == "acme""#).unwrap();
    assert!(rule.eval(&NoInput, &opts).pass());
}

#[test]
fn dynamic_return_types() {
    let pick = Function::new::<KeyArgs, ValueRef>(
        FuncSchema {
            name: "pick",
            doc: "",
        },
        |a| {
            Ok(if a.key == "n" {
                ValueRef::Int(1)
            } else {
                ValueRef::Bool(true)
            })
        },
    );
    assert_eq!(pick.returns(), "any");
    let opts = Opts::builder().function(pick).build().unwrap();
    let rule = rulekit::parse(r#"pick("n") == 1 and pick("b")"#).unwrap();
    assert!(rule.eval(&NoInput, &opts).pass());
}

#[test]
fn opts_validation() {
    let named = |name: &'static str| {
        Function::new::<NoArgs, bool>(FuncSchema { name, doc: "" }, |_| Ok(true))
    };
    assert!(
        Opts::builder()
            .function(named("starts_with"))
            .build()
            .is_err()
    );
    assert!(
        Opts::builder()
            .macro_source("starts_with", "true")
            .unwrap()
            .build()
            .is_err()
    );
    assert!(
        Opts::builder()
            .function(named("m"))
            .macro_source("m", "true")
            .unwrap()
            .build()
            .is_err()
    );
    assert!(
        Opts::builder()
            .function(named("f"))
            .function(named("f"))
            .build()
            .is_err()
    );
    assert!(Opts::builder().macro_source("bad", "a ==").is_err());
}

#[test]
fn macros_expand_and_reject_arguments() {
    let opts = Opts::builder()
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
            .eval(&kv, &opts)
            .pass()
    );
    let rule = rulekit::parse("internal(1)").unwrap();
    let result = rule.eval(&kv, &opts);
    assert!(matches!(result.error(), Some(Error::MacroArgs { .. })));
    let rule = rulekit::parse("nope()").unwrap();
    let opts = Opts::default();
    let result = rule.eval(&kv, &opts);
    assert!(matches!(result.error(), Some(Error::UnknownFunction(name)) if name == "nope"));
}

#[test]
fn functions_can_report_missing_fields_and_errors() {
    let lookup = Function::new::<KeyArgs, &str>(
        FuncSchema {
            name: "lookup",
            doc: "",
        },
        |a| match a.key {
            "known" => Ok("value"),
            "absent" => Err(FnError::missing(["geo.country"])),
            _ => Err(FnError::msg("unsupported key")),
        },
    );
    let opts = Opts::builder().function(lookup).build().unwrap();
    let eval = |expr: &str| {
        let rule = rulekit::parse(expr).unwrap();
        let result = rule.eval(&NoInput, &opts);
        let error = result.error().map(ToString::to_string);
        (
            result.pass(),
            result
                .missing_fields()
                .map(str::to_owned)
                .collect::<Vec<_>>(),
            error,
        )
    };
    assert_eq!(eval(r#"lookup("known") == "value""#), (true, vec![], None));
    assert_eq!(
        eval(r#"lookup("absent") == "x""#),
        (false, vec!["geo.country".to_owned()], None)
    );
    assert_eq!(
        eval(r#"lookup("other") == "x""#),
        (
            false,
            vec![],
            Some(r#"function "lookup": unsupported key"#.to_owned())
        )
    );
}

#[test]
fn question_mark_converts_boxed_errors() {
    fn backend() -> Result<i64, Box<dyn std::error::Error + Send + Sync>> {
        Err("backend down".into())
    }
    let f = Function::new::<NoArgs, i64>(FuncSchema { name: "f", doc: "" }, |_| Ok(backend()?));
    let opts = Opts::builder().function(f).build().unwrap();
    let rule = rulekit::parse("f() == 1").unwrap();
    let result = rule.eval(&NoInput, &opts);
    assert_eq!(
        result.error().map(ToString::to_string).as_deref(),
        Some(r#"function "f": backend down"#)
    );
}

#[test]
fn array_arguments_borrow_input_and_literal_lists() {
    let count = Function::new::<CountArgs, i64>(
        FuncSchema {
            name: "count",
            doc: "",
        },
        |a| Ok(a.list.len() as i64),
    );
    assert_eq!(count.params()[0].ty(), "array");
    let opts = Opts::builder().function(count).build().unwrap();
    let kv = input(&[
        ("tags", Value::Array(vec![Value::Int(1), Value::Int(2)])),
        ("host", Value::String("h".into())),
    ]);
    let rule = rulekit::parse(r#"count(tags) == 2 and count([host, "x", 3]) == 3"#).unwrap();
    assert!(rule.eval(&kv, &opts).pass());

    let rule = rulekit::parse("count(host) == 1").unwrap();
    let result = rule.eval(&kv, &opts);
    assert!(
        matches!(result.error(), Some(Error::InvalidArg { name, expected, got })
        if name == "list" && expected == "array" && got == "string")
    );
}

#[test]
fn macros_carry_docs() {
    let m = rulekit::Macro::new("a == 1").unwrap().with_doc("A is one.");
    assert_eq!(m.doc(), Some("A is one."));
}
