# rulekit (Rust)

Rust implementation of [Rulekit](https://github.com/qpoint-io/rulekit), an
expression-based rules engine:

```perl
domain matches /example\.com$/ and port == 8080
```

The rule language (operators, types, paths, value fields, macros, functions)
is described in the [language reference](../README.md). API documentation:
`cargo doc --open`.

## Install

```toml
[dependencies]
rulekit = { git = "https://github.com/qpoint-io/rulekit", branch = "v2-rust" }
```

Requires Rust 1.88 or later.

## Parse and evaluate

```rust
use rulekit::value::{Map, Value};
use rulekit::{KvInput, Opts};

let rule = rulekit::parse(r"domain matches /example\.com$/ and port == 8080")?;

let input = KvInput::from_values(Map::from_iter([
    ("domain".to_owned(), Value::String("example.com".into())),
    ("port".to_owned(), Value::Int(8080)),
]));

let result = rule.eval(&input, &(), Opts::default());
if let Some(err) = result.error() {
    eprintln!("error evaluating rule: {err}");
} else if result.unknown() {
    eprintln!("missing fields: {:?}", result.missing_fields());
} else if result.pass() {
    println!("PASS!");
} else {
    println!("FAIL :(");
}
```

`Rule::eval(input, ctx, opts)` returns an `EvalResult`:

- `value()`: the result value, usually a boolean
- `error()`: an evaluation error (input or function failure, unknown function, bad argument)
- `missing_fields()`: fields needed to decide that the input lacked
- `trace()`: the evaluation trace, when tracing is on
- `pass()` / `fail()`: completed with a non-zero / zero value
- `ok()` / `complete()`: no error and no missing fields
- `unknown()`: no error, but more input is needed

`ctx` is your evaluation context (for example a request or tenant), passed to
inputs, lazy values, and functions. Use `&()` when there is none.

A parse error reports its line and column, and its `Display` points at the
offending source.

## Input

`KvInput` evaluates against a `Kv` tree: a map of field names to `KvEntry`
values. An entry is plain data (`Value`), a nested map, a nested `Input` that
resolves the rest of the path, or a `Lazy` value computed from the context on
first read:

```rust
use rulekit::value::Value;
use rulekit::{Env, Kv, KvEntry, KvInput, Lazy, Opts};

struct Ctx {
    user: String,
}

let input = KvInput::new(Kv::from_iter([
    ("method".to_owned(), Value::String("GET".into()).into()),
    (
        "user".to_owned(),
        KvEntry::Lazy(Lazy::new(|ctx: &Ctx| Ok(Value::String(ctx.user.clone())))),
    ),
]));

let rule = rulekit::parse(r#"method == "GET" and user == "alice""#)?;
let env = Env::new();
let ctx = Ctx { user: "alice".into() };
assert!(rule.eval(&input, &ctx, Opts::new(&env)).pass());
```

`FnInput` wraps a closure that resolves a path; implement the `Input` trait
for full control. `NoInput` has no fields.

### JSON

`decode_json` turns a JSON object into a `Kv`. Plain JSON decodes
dynamically; `JsonOptions` enables annotated keys (`"src.$ip": "1.2.3.4"`)
or fully typed documents (`{"$type": "ip", "value": "1.2.3.4"}`):

```rust
use rulekit::{JsonOptions, KvInput, Opts, decode_json};

let data = br#"{"src.$ip": "10.1.2.3", "port": 443}"#;
let opts = JsonOptions { annotated_keys: true, ..JsonOptions::default() };
let input = KvInput::new(decode_json::<()>(data, opts)?);

let rule = rulekit::parse("src in 10.0.0.0/8 and port == 443")?;
assert!(rule.eval(&input, &(), Opts::default()).pass());
```

## Functions and macros

Custom functions and macros live in an `Env`, passed with `Opts::new(&env)`.
`EnvBuilder::build` rejects names that shadow the standard library
(`starts_with`) and macros named like a function.

```rust
use rulekit::value::{Val, Value};
use rulekit::{ArgSpec, Env, Function, NoInput, Opts, Type};

let clamp = Function::new(
    [ArgSpec::typed("n", Type::Int64), ArgSpec::typed("max", Type::Int64)],
    |_: &(), args| {
        let n: i64 = args.index(0)?;
        let max: i64 = args.by_name("max")?;
        Ok(Val::Owned(Value::Int(n.min(max))))
    },
);

let env = Env::builder()
    .function("clamp", clamp)
    .macro_source("is_internal", "ip in 10.0.0.0/8")?
    .build()?;

let rule = rulekit::parse("clamp(150, 100) == 100 and not is_internal()")?;
let result = rule.eval(&NoInput, &(), Opts::new(&env));
assert_eq!(result.missing_fields(), ["ip"]);
```

Typed arguments are checked before the function runs. Functions receive the
evaluation context as their first argument and may return values borrowed
from it.

## Tracing

`Opts::with_trace(true)` records how each node evaluated:

```rust
use rulekit::{NoInput, Opts, TraceStatus};

let rule = rulekit::parse("port == 443 or tls")?;
let result = rule.eval(&NoInput, &(), Opts::default().with_trace(true));
let trace = result.trace().unwrap();
assert_eq!(trace.status(), TraceStatus::Missing);
assert_eq!(trace.missing_fields(), ["port", "tls"]);
for child in trace.children() {
    println!("{} -> {}", child.expr(), child.status().name());
}
```

Each `Trace` node has the expression, value, error, missing fields,
comparison diagnostics, a `status` (`passed`, `failed`, `missing`, `error`,
`pruned`, or `unknown`), and its children. Short-circuited branches are
`pruned`; a macro call has the expanded macro as its child.

## Printing and rewriting

`Ast::parse` parses without compiling, for tools; `rulekit::compile` turns an
`Ast` into a `Rule`. An `Ast` exposes its nodes (`root()`, `NodeRef`), tokens
with comment trivia (`tokens()`), and a JSON form (`json()`, or serialize the
`Ast` with serde).

```rust
use rulekit::{Ast, Edit, PrintMode, format, rewrite};

let ast = Ast::parse("a==1&&(b||c)")?;
assert_eq!(format(&ast, &PrintMode::Compact), "a == 1 and (b or c)");
let pretty = format(&ast, &PrintMode::Multiline("  ".into()));

// Replace nodes, keeping the rest of the source (comments included).
let ast = Ast::parse("a == 1   -- first check\nand b")?;
let target = ast.root().children()[0].id();
let replacement = Ast::parse("c == 2")?;
let out = rewrite(&ast, &[Edit { target, replacement: &replacement }], &PrintMode::Compact)?;
assert_eq!(out, "c == 2   -- first check\nand b");
```

`Rule::print(mode)` prints a compiled rule; `Display` on `Rule`, `Ast`, and
`NodeRef` gives the compact canonical expression.

## Go API names

| Go | Rust |
| --- | --- |
| `Parse`, `ParseAST`, `Compile` | `parse`, `Ast::parse`, `compile` |
| `rule.Eval(ctx, input, opts)` | `rule.eval(&input, &ctx, opts)` |
| `Result` | `EvalResult` |
| `Opts{Functions, Macros, Trace}` | `Opts { env, trace }` with `Env::builder()` |
| `FromKV`, `KV` | `KvInput::new`, `Kv` |
| `FromFunc`, `FromContextFunc` | `FnInput` |
| `LazyValue`, `LazyContextValue` | `Lazy` |
| `DecodeJSON`, `JSONOptions` | `decode_json`, `JsonOptions` |
| `MacroSet.Register` | `EnvBuilder::macro_source` |
| `IndexFuncArg` | `Args::by_name` |
| `Format`, `Rewrite`, `Edit` | `format`, `rewrite`, `Edit` |
| `Source()`, `Compact()`, `Multiline(indent)` | `PrintMode::Source`, `Compact`, `Multiline(indent)` |

## License

[MIT](../LICENSE)
