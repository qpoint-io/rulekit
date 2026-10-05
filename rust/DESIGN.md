# rulekit (Rust) design

Status: Phase 0 proposal, for coordinator review. Nothing here is implemented yet.

The Go code at the repo root is the reference. Every semantic rule below is a port of
named Go code. "Mirror Go" means: port that function's behaviour, including its quirks,
unless a QUESTION in the report says otherwise.

## 1. Crate layout

One library crate, `rulekit`, in `rust/`. Edition 2024, `rust-version = "1.85"` (the
first edition-2024 release; the local toolchain is 1.97). No workspace, no cargo features
in v1.

```
rust/
  Cargo.toml            # package.include = ["src/**", "Cargo.toml", "README.md", "LICENSE"]
  src/
    lib.rs              # public re-exports, parse(), compile()
    error.rs            # Error enum, ParseError
    lex.rs              # lexer (port of parser.go lexer half)
    literal.rs          # literal grammar: ints, floats, strings, hex, bools (parser.go)
    parse.rs            # Pratt parser -> AST arena (parser.go parser half)
    ast/
      mod.rs            # Ast, NodeId, NodeRef, Token, Span, AstKind, Operator
      json.rs           # serde Serialize for the JSON AST document (ast_json.go)
    print/
      mod.rs            # canonical printer (printAST), PrintMode
      format.rs         # Compact/Multiline, comment-preserving token formatter (format.go)
      rewrite.rs        # Rewrite by NodeId (rewrite.go)
    value/
      mod.rs            # Value, ValueRef, Val, Map, is_zero, diagnostic type names
      ip.rs             # IP + CIDR parsing (Go net.ParseIP/ParseCIDR rules) and text forms
      mac.rs            # MAC parsing (mac.go) + text form
      url.rs            # URL parse (port of the parts of Go net/url we use) + text form
      query.rs          # form-urlencoded lookup (fields.go queryField/formDecode)
      text.rs           # TextForm: stack buffer or borrowed str (stringable.go)
      fields.rs         # value fields: url.host, ip.version, cidr.prefix, mac.oui (fields.go)
    regex.rs            # dialect check + rewrite + compile (regex.go + brief)
    compile.rs          # AST -> compiled Node tree (ast.go lowerAST)
    eval/
      mod.rs            # Rule::eval entry, root tracer dispatch
      node.rs           # Node enum + per-node eval (nodes.go)
      compare.rs        # compare matrix (compare*.go)
      result.rs         # EvalResult, Missing, error coalescing
      trace.rs          # Tracer trait, NoTrace, Recording, Trace, Diagnostic (trace.go)
    input/
      mod.rs            # Input trait, PathSegment, dyn support, FnInput
      kv.rs             # Kv / KvInput / KvEntry, path traversal (input.go, values.go)
      lazy.rs           # Lazy<C>: memoized, single-flight cell
    func/
      mod.rs            # Function<C>, ArgSpec, Type, Args, FromArg
      stdlib.rs         # starts_with
    macros.rs           # Macro, MacroSet
    env.rs              # Env<C> (validated functions + macros), Opts
    json_input.rs       # decode_json (json.go)
  tests/                # unit-ish integration tests that ship nowhere (see §9)
    vectors.rs          # vector runner; reads ../testdata/vectors
    alloc.rs            # counting-allocator test: happy-path eval allocates 0 bytes
  benches/
    eval.rs             # criterion, mirrors Go benchmarks (see §10)
```

`package.include` whitelists only `src/`, so `tests/`, `benches/` and the vectors (which
live outside `rust/` anyway) never enter the published package.

### Dependencies

| crate | why |
|---|---|
| `regex` | matching engine (brief) |
| `regex-syntax` | parse the pattern to an AST for the dialect checks and rewrites (§5) |
| `serde`, `serde_json` | AST JSON, `decode_json`, vectors (D13: always on) |
| `smallvec` | inline missing-field lists and call-argument buffers (alloc-free hot path) |
| `foldhash` | fast hasher for `Map` (Go's map hashing is far faster than SipHash) |
| `base64` | `$base64` decoding (`STANDARD`, after stripping `\r`/`\n` like Go) |
| dev: `criterion` | benches |

No `url`, no `ipnet`, no `std` IP `Display`/`FromStr` for anything user-visible (brief).

## 2. Values

D6: Go's typed slices existed to avoid reflection. Rust has one array type.

### Owned `Value` (input data, literals, function results, lazy results)

```rust
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Uint(u64),
    Float(f64),
    String(String),
    Bytes(Vec<u8>),          // Go []byte and HexString (both diagnose as "bytes")
    Ip(Ip),                  // IPv4-mapped IPv6 normalized to V4 at construction
    Cidr(Cidr),
    Mac(Mac),                // 6 or 8 bytes, inline
    Url(Box<Url>),           // keeps original text + pre-split, pre-lowercased parts
    Regex(Box<Regex>),       // only from literals / functions
    Array(Vec<Value>),
    Object(Map<Value>),      // Map<V> = HashMap<String, V, foldhash::fast::RandomState>
}
```

`Ip` wraps `std::net::IpAddr` (storage only; text is ours). `Cidr { addr: Ip, prefix: u8,
written_prefix: u8 }` stores the host-bits-cleared network; `written_prefix` keeps Go's
`Mask.Size()` for mapped networks (see QUESTIONS). `Url` stores the original text, the
text form (computed once at parse), and byte ranges for scheme/user/host/port/path/query/
fragment, plus the lowercase host, so every URL field read is a borrow.

### Borrowed `ValueRef<'a>` (what eval works on)

```rust
#[derive(Clone, Copy)]
pub enum ValueRef<'a> {
    Null, Bool(bool), Int(i64), Uint(u64), Float(f64),
    Str(&'a str),
    Query(&'a str),          // Go urlQuery: compares as text, has fields
    Bytes(&'a [u8]),
    Ip(Ip), Cidr(Cidr), Mac(Mac),          // small Copy types, by value
    Url(&'a Url),
    Regex(&'a Regex),
    Array(&'a [Value]),
    Object(ObjectRef<'a>),   // Plain(&'a Map<Value>) | Kv(&'a Kv<..>) erased | Opaque
}
```

Go treats a map, a nested `Input`, or an unknown Go type as "object"/"unknown": truthy,
incomparable. `ObjectRef::Opaque` covers nested inputs reached as a final value.

### `Val<'a>`: borrowed-or-owned

```rust
pub enum Val<'a> { Ref(ValueRef<'a>), Owned(Value) }   // as_ref() -> ValueRef<'_>
```

Inputs, functions and lazies return `Val`; fields from a `Kv` are always `Ref` (D5).
Owned only for: function results, computed lookups (e.g. a repeated URL query parameter
becomes an owned `Array` of strings; `formDecode` that actually decodes), and non-constant
array literals evaluated as a *result* (see §4).

### Text forms (`value/text.rs`)

`TextForm<'a>` = `Borrowed(&'a str)` or `Inline { buf: [u8; 48], len: u8 }`. IPv6 max is
39 bytes, CIDR max 43, MAC max 23; URL text is precomputed and borrowed. So stringable
comparisons and `starts_with` never allocate. Algorithms are hand-written to the table in
`testdata/vectors/README.md` (RFC 5952 longest-run/first-on-tie, no single-group `::`, no
embedded dotted form, mapped → IPv4, mapped CIDR → IPv4 with prefix − 96, lowercase MAC).

### Parsing typed values

- IP / CIDR: hand port of Go `net.ParseIP` / `net.ParseCIDR` acceptance rules (IPv4 octets
  without leading zeros, `::` rules, embedded IPv4 tail, no zones; CIDR prefix decimal ≤
  bits). Used by the lexer too, so token classification matches Go exactly.
- MAC: port of `mac.go`.
- URL: port of Go `net/url.Parse` (the subset reachable: scheme, opaque, userinfo, host
  incl. `[v6]:port`, path/RawPath validity, query, fragment, and its error cases) and of
  `URL.String()` / `EscapedPath()` / `Hostname()` / `Port()`. Go's behaviour *is* the text
  form ("as written, scheme+host lowercased, disallowed chars percent-encoded"), so porting
  is lower-risk than re-deriving it.

## 3. AST

D10/D11: one arena AST, private fields, public accessors.

```rust
pub struct Ast { source: String, nodes: Vec<NodeData>, root: NodeId, tokens: Vec<Token> }
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)] pub struct NodeId(u32);

enum NodeData {           // children are NodeIds into the arena
    Literal { span, kind: LitKind, raw: Range },          // raw = byte range into source
    Path    { span, segments: Box<[Segment]> },
    Array   { span, items: Box<[NodeId]> },
    Call    { span, name: Range, args: Box<[NodeId]> },
    Unary   { span, op: Operator, raw_op: Range, operand: NodeId },
    Binary  { span, op: Operator, raw_op: Range, negated: bool, lhs: NodeId, rhs: NodeId },
}

pub struct NodeRef<'a> { ast: &'a Ast, id: NodeId }
impl NodeRef<'_> { id, kind, span, children, operator, raw_operator, negated,
                   literal_raw, path, call_name, to_string /* compact canonical */ }
pub struct Token { kind: TokenKind, raw: Range, span: Span, leading: Range, trailing: Range }
```

- `Ast::parse(&str) -> Result<Ast, ParseError>`, `Ast::root()`, `Ast::node(id)`,
  `Ast::tokens()` (with EOF), `Ast::source()`, `Display` = canonical string.
- Go mutates `astPath.segments` in `parsePostfix`; the Rust parser builds paths in a local
  `Vec<Segment>` and pushes one arena node when the postfix loop ends, so arena ids stay
  dense and children precede parents.
- JSON (`ast/json.rs`): a hand `impl Serialize for Ast` that emits exactly `JSONAST`:
  `source`, `root` (ids `root`, `root.0`, …; `operator`/`negated`/`raw`/`path` with Go's
  `omitempty` semantics, `children` omitted when empty), `tokens` without EOF, spans with
  1-based line and byte column (port `lineIndex`). Vectors compare as JSON values, but key
  order will match Go's struct order anyway. A `JsonAst` owned struct (`Ast::json()`)
  mirrors Go's `AST.JSON()`.
- Tree-position ids ("root.0.1") are a JSON concern only; the API identifies nodes by
  `NodeId`. `NodeRef::tree_id()` returns the string form for tools that want it.

## 4. Compiled rules

```rust
pub struct Rule { ast: Arc<Ast>, root: Node }      // Send + Sync, Clone
pub fn parse(src: &str) -> Result<Rule, ParseError>   // Ast::parse + compile
pub fn compile(ast: Arc<Ast>) -> Result<Rule, ParseError>

struct Node { kind: NodeKind, meta: Meta }
struct Meta { id: NodeId, kind: AstKind, expr: Box<str> }   // trace metadata (D3)

enum NodeKind {
    And(Box<Node>, Box<Node>),
    Or(Box<Node>, Box<Node>),
    Not(Box<Node>),
    Compare { op: CmpOp, lhs: Box<Node>, rhs: Box<Node> },   // == != > >= < <= contains, and `in CIDR`
    Match   { lhs: Box<Node>, rhs: Box<Node> },              // rhs is always a regex literal
    In      { lhs: Box<Node>, rhs: Box<Node> },              // rhs is always an Array node
    Literal(Value),
    Field(Box<str>),                                          // single plain segment
    Path { segments: Box<[Segment]>, text: Box<str> },        // text = missing-field name
    Array(Box<[Node]>),
    ConstArray(Box<[Value]>),                                 // all items literal: built once
    Call(Call),                                               // Stdlib(StartsWith) | Named(Box<str>)
    Custom(Box<dyn CustomRule>),
}
```

- Lowering is a port of `lowerAST`, including: `in` with a CIDR literal → `Compare(EQ)`;
  negated binaries → `Not(base)` with one `Meta` for the pair (Go wraps once); literal
  value errors are reported as `ParseError` at the literal span.
- Literal regexes are compiled at compile time and live in `Literal(Value::Regex)`.
- `Meta.expr` is precomputed (Go also precomputes `tracedRule.expr`). Compiled nodes have
  no printer of their own: Go's per-node `String()` is only reachable through traces, which
  always use the AST text.
- Array operands: `Compare`/`In` whose operand is an `Array` node evaluate the items into a
  stack `SmallVec<[Val; 8]>` and compare in place, preserving Go's "evaluate every item
  first, first non-ok item wins" semantics. Only an array that is itself a rule's (or a
  function argument's) *value* materializes an owned `Value::Array`, as Go does.
- `Custom(Box<dyn CustomRule>)` keeps user-defined rules possible (D3). `CustomRule` is
  object safe and receives `&dyn Input<C>`-style erased arguments; since `Rule` is not
  generic over `C`, the custom trait takes `&dyn Any` for the context (see QUESTIONS).
- `Rule::print(mode)`, `Display for Rule` (canonical), `Rule::ast()`.

## 5. Regex dialect (`regex.rs`)

Pipeline for a regex literal `/pat/flags` (port of `parseRegex` + brief):

1. Duplicate-flag check; prefix `(?flags)`; flags limited to `i`, `m`, `s` by the lexer.
2. `check_regex_dialect`: byte-for-byte port of Go `checkRegexDialect` (same scan, same
   error cases).
3. Parse with `regex_syntax::ast::parse::Parser` (nest limit tuned to Go's). Walk the AST
   and reject what Go's RE2 parser rejects but `regex-syntax` accepts:
   - flags other than `i m s U` and `-` in groups, i.e. `(?x)`, `(?R)`, `(?u)`;
   - nested repetition (`a**`, `a+*`, `a{2}{3}`; laziness `?` is not a repetition);
   - counted repetition with min or max > 1000 (Go `errInvalidRepeatSize`), and Go's
     total-size check for nested counted repetitions;
   - escapes Go lacks: `\u`, `\U`, `\e`(none in Go), `\b{…}` (already caught by step 2),
     `\z`-family differences, special word boundaries `\<`/`\>` (step 2);
   - Unicode class names not in Go's `unicode.Categories`/`unicode.Scripts` + `Any`,
     including `name=value` / `sc=` forms and loose matching (Go is exact-case, no
     spaces/underscores folding);
   - unknown POSIX classes `[[:foo:]]`.
4. Rewrite (span splicing on the original pattern, from the AST positions):
   - `\d` → `[0-9]`, `\D` → `[^0-9]`;
   - `\w` → `[0-9A-Za-z_]`, `\W` → `[^0-9A-Za-z_]`;
   - `\s` → `[\t\n\f\r ]`, `\S` → `[^\t\n\f\r ]`;
   - inside a class the positive forms splice ranges in place; negated forms become a
     nested negated class (legal in Rust, never visible to users);
   - `\b` → `(?-u:\b)`, `\B` → `(?-u:\B)`.
   Using explicit ASCII classes rather than `(?-u:\w)` keeps Go's `(?i)` behaviour: Go case
   folds Perl classes under `(?i)` (so `(?i)\w` matches U+212A KELVIN SIGN), and so does
   `regex` for an explicit class, but not for `(?-u:\w)`. Phase 1 verifies this with tests.
5. `regex::RegexBuilder` compile with Unicode on, default size limits. Errors at any step
   are literal parse errors.

The Unicode property table is a static sorted `&[&str]` in `regex.rs`, listing exactly
the names Go's `unicode` package exposes for the Go version the vectors are run with (see
QUESTIONS on how to generate it).

## 6. Input, lazy values, Ctx

D2/D4/D9. Eval is generic over a caller context `C` (default `()`).

```rust
pub enum PathSegment<'p> { Key { key: &'p str, bracket: bool }, Index(usize) }

pub trait Input<C: ?Sized = ()> {
    fn get<'a>(&'a self, ctx: &'a C, path: &[PathSegment<'_>]) -> Result<Option<Val<'a>>, Error>;
}
impl<C: ?Sized, T: Input<C> + ?Sized> Input<C> for &T { … }   // so &dyn Input<C> works
pub struct NoInput;                                            // Go's nil input
pub fn from_fn<C, F>(f: F) -> impl Input<C>                    // Go FromFunc/FromContextFunc
```

The trait method has no generics, so `dyn Input<C> + Send + Sync` works; eval takes
`I: Input<C> + ?Sized`.

### `Kv` / `KvInput` (port of `kvInput`)

```rust
pub struct Kv<C = ()>(Map<KvEntry<C>>);
pub enum KvEntry<C = ()> {
    Value(Value),
    Object(Kv<C>),
    Lazy(Lazy<C>),
    Input(Arc<dyn Input<C> + Send + Sync>),
}
pub struct KvInput<C = ()> { kv: Kv<C> }      // owns the tree
impl<C> Input<C> for KvInput<C> { … }
```

- Traversal ports `kvInput.GetPath` + `indexPath`: map key lookup, index into arrays,
  value fields on typed values (`fields.rs`), hand-off of the remaining path to a nested
  `Input`, lazy resolution only for map entries. Lazy results are `KvEntry<C>`, so a lazy
  may return a subtree containing more lazies or inputs, as in Go.
- `Lazy<C>` holds `Arc<dyn Fn(&C) -> Result<KvEntry<C>, BoxError> + Send + Sync>`, a
  `OnceLock<KvEntry<C>>` and a `Mutex<()>`. Resolution: `OnceLock::get()` (lock-free fast
  path) → lock the entry mutex → re-check → call → store on success only. That is
  single-flight per path, errors not memoized, and lazies on other paths never block each
  other. Plain values never touch a lock.
- Per-input-instance memo: Go keys the memo by path string inside each `kvInput`. Here the
  memo cell sits on the lazy entry itself and `KvInput` owns its tree; `Clone` for `Lazy`
  produces a fresh, unresolved cell. A path string identifies exactly one map location,
  so this is the same memo, with no path-string hashing and no input-wide lock. Resolved
  values are borrowed from the cell for the input's lifetime.
- Go has two lazy types (`LazyValue`, `LazyContextValue`); Rust has one that receives
  `&C` (ignore it if unused).
- Arrays inside a `Kv` hold plain `Value`s only (see QUESTIONS: Go allows an `Input` as a
  `[]any` element).

## 7. Eval, results, options

```rust
impl Rule {
    pub fn eval<'a, C, I>(&'a self, input: &'a I, ctx: &'a C, opts: &Opts<'a, C>) -> EvalResult<'a>
    where I: Input<C> + ?Sized;
}

pub struct Opts<'e, C = ()> { pub trace: bool, pub env: &'e Env<C> }  // Copy; Default uses an empty Env
pub struct Env<C = ()> { functions: Map<Function<C>>, macros: Map<Macro> }
pub struct EnvBuilder<C> { … }   // .function(name, f) .macro_(name, src) .build() -> Result<Env<C>, Error>
```

- D8: `EnvBuilder::build` runs Go's `Opts.Validate` checks once (name conflicts with stdlib,
  macro vs function conflicts). `Env` cannot be built unvalidated, so eval never validates.
  `trace` is per call and lives in the cheap `Opts`, matching Go's `Opts.Trace` (D7).
- One lifetime `'a` covers rule, input, ctx and env so results can borrow from any of them.

```rust
pub struct EvalResult<'a> {
    value: Val<'a>,                    // Null = no value (Go nil)
    error: Option<Error>,
    missing: SmallVec<[&'a str; 2]>,   // borrowed path texts; order = Go's unionUnique order
    trace: Option<Box<Trace>>,
}
// pass(), fail(), ok()/complete(), unknown(), value(), error(), missing_fields(), trace(), into_owned()
```

- `Error` (D12) is an enum: `Parse(ParseError)`, `UnknownFunction`, `ArgCount`,
  `InvalidArg { name, expected, got }`, `MacroArgs`, `Input { field, source }`,
  `Function(BoxError)`, `Multiple(Vec<Error>)` (Go `coalesceErrs`), `Env(..)`, `Json(..)`.
- Node semantics are a line-by-line port of `nodes.go` (And/Or short-circuit and
  ok/not-ok merging, Not, Match, Compare, In) and `compare*.go` (the full type matrix,
  `compareSliceDetailed` NE/CONTAINS rules, diagnostics). Numbers port `cmpNumber` incl.
  `cmp.Compare` NaN ordering and lossy int→float conversion.
- `is_zero` ports `isZero` (object/opaque/regex are truthy).

## 8. Tracing (D7)

```rust
trait Tracer { type Frag; … }        // NoTrace: Frag = ();  Recording: Frag = Option<Box<Trace>>
```

`Rule::eval` branches once on `opts.trace` into `eval_node::<NoTrace>` or
`eval_node::<Recording>`; the untraced instantiation has no per-node trace code. The
`Recording` tracer reproduces Go's trace *shape* exactly — `combineTrace` anonymous nodes,
`tracedRule` taking `traceChildren`/`traceDiagnostics` of the inner trace, pruned siblings
from `prunedTrace`, the macro wrapper in `FunctionValue.Eval` — because vectors compare
exact child lists.

```rust
pub struct Trace {
    pub node: Option<NodeId>, pub kind: Option<AstKind>, pub expr: String,
    pub value: Value, pub error: Option<String>, pub missing_fields: Vec<String>,
    pub diagnostics: Vec<Diagnostic>, pub status: TraceStatus,
    pub active: bool, pub pruned: bool, pub children: Vec<Trace>,
}
```

Traces are owned (values cloned): tracing is the debug path. `node` is an id into the
rule's (or macro's) AST; `kind` is stored so consumers need not resolve the AST.

## 9. Functions and macros (D14, D15)

```rust
pub struct Function<C = ()> {
    args: Box<[ArgSpec]>, ret: Option<Type>, doc: Option<String>,
    eval: Arc<dyn for<'a> Fn(&'a C, Args<'_, 'a>) -> Result<Val<'a>, Error> + Send + Sync>,
}
pub struct ArgSpec { pub name: Cow<'static, str>, pub ty: Option<Type> }
pub enum Type { Any, Null, Bool, Int64, Uint64, Float64, String, Bytes, Ip, Cidr, Mac, Url, Regex, Array, Object }

pub struct Args<'s, 'a> { spec: &'s [ArgSpec], vals: &'s [Val<'a>] }
impl<'a> Args<'_, 'a> {
    pub fn len(&self) -> usize;
    pub fn get(&self, i: usize) -> ValueRef<'_>;                 // panics past len (count is pre-validated)
    pub fn named(&self, name: &str) -> Option<ValueRef<'_>>;     // linear scan over specs
    pub fn index<T: FromArg>(&self, i: usize) -> Result<T, Error>;   // typed, InvalidArg on mismatch
    pub fn by_name<T: FromArg>(&self, name: &str) -> Result<T, Error>;
}
```

- Positional args evaluated into a stack `SmallVec<[Val; 4]>`: no map, no allocation.
  Arg count checked before evaluating (Go order); declared `ty` checked before calling.
- `starts_with` is resolved at compile time to `Call::Stdlib(StartsWith)`; parse-time arity
  error for stdlib functions ports `parseFunction`. Other names resolve at eval: custom
  function, then macro, else `UnknownFunction` (Go order).
- `starts_with` uses `TextForm` for both args (strings or text-form types; anything else
  is `InvalidArg`), so it never allocates.
- `Macro { source, ast: Arc<Ast>, rule: Rule }` (non-generic). Macro calls with args are
  an error, as in Go.
- No reflection-based `Func` builder (Go Phase 13 sketch has one); closures plus `FromArg`
  cover it.

## 10. JSON input (`json_input.rs`)

`decode_json<C>(bytes, JsonOptions { annotated_keys, typed_document }) -> Result<Kv<C>, Error>`
ports `json.go` exactly. Parsing uses `serde_json` with a small custom `Deserialize`
visitor (not `serde_json::Value`) so numbers follow Go's `UseNumber` rules: integers that
fit i64 → `Int`, else u64 → `Uint`, fraction/exponent → `Float`. This avoids enabling
serde_json's `arbitrary_precision`, a feature that unifies into the user's whole
dependency graph. Known gap: serde_json hands an integer literal beyond u64 to the visitor
as f64, where Go errors ("invalid json number"); see QUESTIONS. Also ported: duplicate-key
detection after suffix stripping, the suffix list order, typed-document rules, `hex` with
colons stripped, base64 Std with Go's `\r`/`\n` skipping. `decode_json` lands in Phase 2
because the eval vectors need it.

## 11. Vectors (`tests/vectors.rs`)

- Reads every `*.json` in `$CARGO_MANIFEST_DIR/../testdata/vectors`; any other entry except
  `README.md` fails the run.
- Strict decoding: serde structs with `deny_unknown_fields`; `input`, `value`, `ast_json`
  stay `serde_json::Value`; duplicate case names fail.
- Steps in README order: decode (`decode_json` in the case's mode) → parse
  (`Ast::parse` + `compile`; `parse_error` with line/column checks `ParseError`) → print
  (`string`, `compact`, `source`, `multiline_2sp/4sp`, plus reparse round trips) →
  `ast_json` (`serde_json::to_value(&ast) == expected`) → eval twice (trace off / on, untraced
  must have no trace), checking `error`, sorted `missing_fields`, `value`, `trace`.
- Expected values are decoded with the typed-JSON shorthand into `Value` and compared with
  a test-only equality: type + content, bytes by content, network values by text form.
- One `#[test]` per vector file, reporting every failing case name (not just the first).

## 12. Benchmarks (`benches/eval.rs`, criterion)

Mirror the Go benchmarks one to one, same names, same expressions and inputs:

- `eval/<case>` for every `BenchmarkEval` case (short-circuit string, string slice, full
  traversal pass/no-match, nested path number, bracket path, array index, regex, ip_cidr,
  missing_fields, function, macro);
- `eval_lazy_input/{pruned,resolved_cached}`;
- `eval_trace`;
- `parse/{simple,complex}`;
- `cmp_number/<T>-<U>` over i64/u64/f64.

Plus `tests/alloc.rs`: a counting `#[global_allocator]` asserts zero allocations for every
`BenchmarkEval` case except `missing_fields` (missing list stays inline up to 2; the case
has 2) — goal is zero there too. A script `rust/bench/compare.sh` runs
`go test -bench 'Eval|Parse|CmpNumber' -benchmem` and `cargo bench`, and prints a
side-by-side ns/op + allocs table. Release profile for benches: `lto = "fat"`,
`codegen-units = 1`. Compiled size of the two tracer instantiations measured with
`cargo bloat` when tracing lands (D7).

## 13. Phase plan (unchanged from brief)

1. lexer/parser/AST/printer + AST JSON vectors (+ parse_errors, print, literals parse cases)
2. values, text forms, compare, eval + eval/literals/paths/fields/text_forms/regex vectors
3. input/lazy, 4. functions/macros, 5. tracing, 6. format/rewrite, 7. decode_json, 8. benches.
