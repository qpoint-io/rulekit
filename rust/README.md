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
rulekit = { git = "https://github.com/qpoint-io/rulekit", branch = "v2" }
```

Requires Rust 1.88 or later.

## Parse and evaluate

```rust
use rulekit::Opts;

#[derive(rulekit::Input)]
struct Request<'a> {
    domain: &'a str,
    port: u16,
}

let rule = rulekit::parse(r"domain matches /example\.com$/ and port == 8080")?;
let input = Request { domain: "example.com", port: 8080 };

let result = rule.eval(&(), &input, Opts::default());
if let Some(err) = result.error() {
    eprintln!("error evaluating rule: {err}");
} else if result.unknown() {
    eprintln!("missing fields: {:?}", result.missing_fields().collect::<Vec<_>>());
} else if result.pass() {
    println!("PASS!");
} else {
    println!("FAIL :(");
}
```

`Rule::eval(ctx, input, opts)` returns an `EvalResult`:

- `value()`: the result value, usually a boolean
- `error()`: an evaluation error (input or function failure, unknown function, bad argument)
- `missing_fields()`: an iterator over the fields needed to decide that the input lacked
- `trace()`: the evaluation trace, when tracing is on
- `pass()` / `fail()`: completed with a non-zero / zero value
- `ok()` / `complete()`: no error and no missing fields
- `unknown()`: no error, but more input is needed

`ctx` is your evaluation context (for example a request or tenant), passed to
inputs, lazy values, and functions. Use `&()` when there is none.

A parse error reports its line and column, and its `Display` points at the
offending source.

## Input

Pass a `#[derive(rulekit::Input)]` struct, a `kv!` map, a `HashMap`/`BTreeMap`
with string keys, or a `serde_json::Value`. A rule reads only the fields it
names; the rest of a large input is not walked or copied. Strings are borrowed
from your data.

```rust
use std::collections::HashMap;
use rulekit::{Opts, kv, lazy};

struct Ctx {
    user: String,
}

#[derive(rulekit::Input)]
struct Request<'a> {
    method: &'a str,
    headers: &'a HashMap<String, String>,
    user: rulekit::Lazy<Ctx>,
}

let headers = HashMap::from([("host".into(), "example.com".into())]);
let input = Request {
    method: "GET",
    headers: &headers,
    user: lazy(|ctx: &Ctx| Ok(ctx.user.clone())),
};

let rule = rulekit::parse(
    r#"method == "GET" and headers.host == "example.com" and user == "alice""#,
)?;
let env: rulekit::Env<Ctx> = rulekit::Env::new();
let ctx = Ctx { user: "alice".into() };
assert!(rule.eval(&ctx, &input, Opts::new(&env)).pass());
```

`#[rulekit(rename = "x")]` changes the name a rule sees; `#[rulekit(skip)]`
omits a field. `#[rulekit(context = Ctx)]` on the struct sets the context
type when it is not `()`. An unknown field is missing. `None` is missing.

`kv!` is the ad-hoc form. A nested `{ ... }` is another map. [`lazy`] runs the first time the field is read, then reuses that owned
value (a borrow of `ctx` is copied into the memo):

```rust
use rulekit::{Opts, kv};

let input = kv! {
    "host" => "api.acme.com",
    "port" => 8443,
    "user" => { "id" => 42 },
};
let rule = rulekit::parse(r#"host == "api.acme.com" and port == 8443 and user.id == 42"#)?;
assert!(rule.eval(&(), &input, Opts::default()).pass());
```

List membership is `tags contains "db"` (`in` takes an array or CIDR literal,
as in Go). `Vec<u8>` and `&[u8]` are lists of numbers, not byte strings; use
`Value::Bytes` for bytes. `url::Url` and `http::Uri` fields (`host`, `path`,
`query.env`, ...) work with the default `url` and `http` features.
`http::Uri` compared as a whole URL allocates its text form; field access does
not.

`FnInput` wraps a closure that resolves a path. `NoInput` has no fields.
`Kv` / `KvInput` / `Value` remain for annotated JSON and other owned trees.

### JSON

A `serde_json::Value` is walked lazily, borrowing strings from the tree.
`decode_json` is the annotated form (`"src.$ip": "1.2.3.4"`, or
`{"$type": "ip", "value": "1.2.3.4"}`), and still yields a `Kv`:

```rust
use rulekit::{JsonOptions, KvInput, Opts, decode_json};

let data = br#"{"src.$ip": "10.1.2.3", "port": 443}"#;
let opts = JsonOptions { annotated_keys: true, ..JsonOptions::default() };
let input = KvInput::new(decode_json::<()>(data, opts)?);

let rule = rulekit::parse("src in 10.0.0.0/8 and port == 443")?;
assert!(rule.eval(&(), &input, Opts::default()).pass());
```

## Functions and macros

Custom functions and macros live in an `Env`, passed with `Opts::new(&env)`.
`EnvBuilder::build` rejects names that shadow the standard library
(`starts_with`) and macros named like a function.

```rust
use rulekit::{Env, FuncSchema, Function, NoInput, Opts};

// One field per positional argument, in order.
#[derive(rulekit::Args)]
struct ClampArgs {
    n: i64,
    max: i64,
}

let clamp = Function::new::<ClampArgs, i64>(
    FuncSchema { name: "clamp", doc: "The smaller of n and max." },
    |_: &(), a| Ok(a.n.min(a.max)),
);

let env = Env::builder()
    .function(clamp)
    .macro_source("is_internal", "ip in 10.0.0.0/8")?
    .build()?;

let rule = rulekit::parse("clamp(150, 100) == 100 and not is_internal()")?;
let result = rule.eval(&(), &NoInput, Opts::new(&env));
assert_eq!(result.missing_fields().collect::<Vec<_>>(), ["ip"]);
```

`#[derive(rulekit::Args)]` (the default `derive` feature) turns a struct into a
function's arguments: each field is one positional argument named after the
field (`#[rulekit(rename = "...")]` to change it), of any `FromArg` type
(`bool`, `i64`, `u64`, `f64`, `&str`, `&[u8]`, `Ip`, `Cidr`, `Mac`, `&Url`,
`TextForm`, or `ValueRef` for any value). The struct may have one lifetime
for borrowed arguments, and a last `Rest<'a>` field for the remaining
arguments; `NoArgs` is the argument type of a function without arguments.
`Function::new::<A, R>` names the argument struct and the return type
(`bool`, `i64`, `&str`, `String`, `Ip`, ..., or `ValueRef`/`Val` when the
type is decided at run time), so the closure needs no annotations and may
return data borrowed from the context. `FuncSchema` holds the name and doc.

Wrong argument counts and types are errors. Functions receive the evaluation
context and their arguments, not the rule's input. A function returns
`Err(FnError::missing([...]))` when it needs input that is not there (the rule
result is then unknown), or any other error with `?` or `FnError::msg`. Macros
take no arguments; use a function for parameterized logic.

## Tracing

`Opts::with_trace(true)` records how each node evaluated:

```rust
use rulekit::{NoInput, Opts, TraceStatus};

let rule = rulekit::parse("port == 443 or tls")?;
let result = rule.eval(&(), &NoInput, Opts::default().with_trace(true));
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
| `rule.Eval(ctx, input, opts)` | `rule.eval(&ctx, &input, opts)` |
| `Result` | `EvalResult` |
| `Opts{Functions, Macros, Trace}` | `Opts { env, trace }` with `Env::builder()` |
| struct / map literal | `#[derive(Input)]`, `kv!` |
| `FromKV`, `KV` | `KvInput::new`, `Kv` (owned escape hatch) |
| `FromFunc`, `FromContextFunc` | `FnInput` |
| `LazyValue`, `LazyContextValue` | `lazy`, `Lazy` |
| `DecodeJSON`, `JSONOptions` | `decode_json`, `JsonOptions` |
| `MacroSet.Register` | `EnvBuilder::macro_source` |
| `rulekit.Func` with an args struct | `Function::new::<A, R>(FuncSchema { name, doc }, closure)` with `#[derive(rulekit::Args)]` |
| `Format`, `Rewrite`, `Edit` | `format`, `rewrite`, `Edit` |
| `Source()`, `Compact()`, `Multiline(indent)` | `PrintMode::Source`, `Compact`, `Multiline(indent)` |

## License

[MIT](../LICENSE)
