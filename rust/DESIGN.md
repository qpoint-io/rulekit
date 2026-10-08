# rulekit (Rust) design

This document records the implemented parity, ownership, and performance decisions.
The Go code at the repo root is the semantic reference; shared vectors define the
behavior supported by both implementations. API signatures and examples live in
[README.md](README.md) and the source rustdoc, not in proposal pseudocode here.

The implementation uses established crates and std rather than hand ports of Go
standard-library code. Superseded implementations are available in git history.

## Input values (implemented)

Users pass their own types: `#[derive(Input)]` on a struct or enum, `kv!`
for an ad-hoc map, or a `HashMap` / `BTreeMap` / `serde_json::Value` / `Map`.
`InputValue` is the field trait. An empty path is the value; a longer path
reads only those segments. Lists and objects are `&dyn` sources
(`ArrayRef::List`, `ObjectRef::Source`), so `tags contains "x"` and
`headers["k"]` do not copy the collection. A borrowed `&[T]` is handed out as
`&&[T]`: a slice is unsized and cannot itself be `&dyn`.

Rejected: implementing input via `serde::Serialize`. Serde walks every field
in order and returns strings with a lifetime that ends when the serializer
call returns, so strings would be copied and unread fields would still be
visited.

### `derive(Input)` follows serde (decision)

Decision: the derive reads `#[serde(...)]` attributes so a rule path is the
path in the value's `serde_json` form, as schemars' `JsonSchema` derive
does for schemas. Filters over typed events (qcontrol's `qevents`) then use
the wire names users see, while reading the typed value lazily: only the
segments a rule names are resolved, strings are borrowed, and nothing
allocates. The derive only parses attributes; serde is not called. The
invariant is tested in `tests/derive_serde.rs`: for qevents-shaped values,
every path of `serde_json::to_value(v)` (and one step past each) reads the
same through `v`.

The derive implements `Input` and `InputValue`, so derived types nest.

Supported:

- Names: container, variant, and field `rename` (including
  `rename(serialize = ...)`), `rename_all` on structs (fields), enums
  (variants), and variants (their fields), `rename_all_fields` on enums. The
  case rules are copied from `serde_derive`. Raw identifiers are unrawed.
- Enum representations: externally tagged (unit variant = its name string,
  others `{"Name": content}`), internally tagged `tag` (tag key = variant
  name, other keys = struct fields or newtype content; unit = only the tag),
  adjacently tagged `tag` + `content` (unit = only the tag), `untagged` on
  the enum or per variant. Variant content: unit `null`, newtype = inner,
  tuple = array, struct = object. `tag` on a named struct adds its name.
- `flatten`: keys the container's named fields (and tag) do not match go to
  flattened fields in declaration order; a flattened `None` contributes
  nothing.
- `transparent`, newtype structs, tuple structs (array), unit structs (`null`).
- `skip`, `skip_serializing`: missing; tuple indices skip them as serde does.
  A skipped variant (serde refuses to serialize it) is missing at every path.
- `skip_serializing_if`: the predicate is called with `&field` when the field
  is read, exactly as serde calls it. When it holds the key is missing and
  falls through to flattened fields.
- `#[rulekit(rename)]` wins over serde's name; `#[rulekit(bytes)]` works on any
  field, including variant fields.

A field serde writes whose value is absent (`None`) reads as `null`: serde
writes `null`, and Go treats a present nil as a value, not missing. A path
below it is missing, as below a JSON `null`. A bare `Option` input value
(`impl InputValue for Option`) is unchanged: `None` is missing. Which types
are nullable is the associated const `InputValue::ABSENT_IS_NULL` (`true` for
`Option`, forwarded by `&T`, `Box`, `Rc`, `Arc`; default `false`), so for every
other field type the derive's field read is the bare `get` call.

Ignored (deserialization only): `default`, `deserialize_with`, `alias`,
`other`, `bound`, `deny_unknown_fields`, `from`, `try_from`,
`skip_deserializing`, `borrow`, `crate`, `expecting`, and any other attribute
serde validates itself.

Rejected with a compile error at the attribute: `serialize_with`, `with`
(field or variant), `getter`, `into`, `remote`. The derive cannot see what a
custom serializer writes; no rulekit attribute overrides this, since without
`#[rulekit(skip)]` there is nothing to substitute. The fix is a field type
whose `Serialize` and `InputValue` impls agree. `#[serde(tag)]` with tuple
variants and conflicting representations are errors, as in serde.

Not mirrored exactly:

- A struct, map-like variant, or tuple read whole is `ObjectRef::Opaque`,
  where `serde_json::Value` gives an object or array source. Keys and indexes
  into it match.
- A named field and a flattened entry with the same key: the named field
  wins; serde writes the key twice (and `serde_json::Value` keeps the last).
- Serialization errors (a flattened non-map, an internally tagged newtype of
  a non-map) are not detected; the derive reads what it can.
- A type alias of `Option` behaves like `Option`; nothing here is syntactic.

`Vec<u8>`, `&[u8]`, `[u8; N]`, `Box<[u8]>`, and `Cow<[u8]>` are lists of
numbers unless the field is `#[rulekit(bytes)]` (borrowed `ValueRef::Bytes`).
`kv!` uses `rulekit::bytes(&buf)`. `bytes::Bytes` / `BytesMut` and
`serde_bytes::{ByteBuf, Bytes, ByteArray}` are byte strings automatically
(default features `bytes` and `serde_bytes`).

The `url` and `http` features are on by default. `url::Url` and `http::Uri`
behave as rulekit's URL parsed from `as_str()` / the display form after those
crates normalize; the original spelling is not recoverable. Field access and
URI comparison borrow.

## 1. Crate layout and dependencies

The `rulekit` library and `rulekit-macros` proc-macro crate form the workspace in
`rust/`. Edition, minimum Rust version, dependencies, package contents, and optional
features are defined in [Cargo.toml](Cargo.toml). The default features support
derives, `url`, `http`, `bytes`, and `serde_bytes`. Tests, benchmarks, and shared
vectors are not part of the published library package.

Source entry points:

- [src/lib.rs](src/lib.rs): parsing, compilation, `Rule`, and public re-exports.
- [src/ast/](src/ast/), [src/parse.rs](src/parse.rs), [src/lex.rs](src/lex.rs),
  [src/literal.rs](src/literal.rs): source spans, arena AST, and syntax.
- [src/print/](src/print/): canonical printing, formatting, and rewriting.
- [src/value/](src/value/): owned/borrowed values, typed values, fields, and text forms.
- [src/eval/](src/eval/): lowering, evaluation, comparison, results, and traces.
- [src/input.rs](src/input.rs), [src/input_value.rs](src/input_value.rs),
  [src/kv.rs](src/kv.rs), [macros/src/](macros/src/): lazy path access and derives.
- [src/func.rs](src/func.rs), [src/stdlib.rs](src/stdlib.rs),
  [src/env.rs](src/env.rs): typed functions, macros, and validated environments.
- [src/regex/](src/regex/), [src/json_input.rs](src/json_input.rs): shared regex
  dialect and JSON semantics.

Key dependencies serve narrow roles: `regex`/`regex-syntax` for the regex engine
and dialect validation, `ipnet` with std IP types for network values, `fluent-uri`
for URI parsing without normalization, `base64` for padded standard decoding,
`serde`/`serde_json` for AST JSON and input decoding, `smallvec` for inline
evaluation buffers, and `foldhash` for the string-keyed `Map`.

## 2. Values

D6: Go's typed slices existed to avoid reflection. Rust has one array type.

### Owned `Value` (input data, literals, function results, lazy results)

`Value` owns input data, literals, and function/lazy results. See
[src/value/mod.rs](src/value/mod.rs) for its variants.

`Ip` wraps `std::net::IpAddr` (parsing and text from std; IPv4-mapped canonicalized
to V4). `Cidr` wraps `ipnet::IpNet` and stores the host-bits-cleared network.
An IPv4-mapped network is stored as the IPv4 network it stands for
(`::ffff:10.0.0.0/104` = `10.0.0.0/8`, prefix 8), matching Go's text, version,
`prefix` field, and containment behavior. `Url` stores its normalized text and
byte ranges for its fields, so field reads borrow rather than reconstruct strings.

### Borrowed `ValueRef<'a>` (what eval works on)

`ValueRef` borrows strings, byte strings, URLs, regexes, and collections; small
copyable network values are held by value. `ArrayRef` supports owned value slices,
evaluation buffers, and caller-provided list sources; `ObjectRef` supports maps,
caller-provided object sources, and opaque objects.

Go treats a map, a nested `Input`, or an unknown Go type as "object"/"unknown": truthy,
incomparable. `ObjectRef::Opaque` covers nested inputs reached as a final value.

### `Val<'a>`: borrowed-or-owned

`Val` holds either a borrowed `ValueRef` or an owned `Value`. Input map values
are borrowed. Ownership is needed for function results, computed lookups (such
as repeated URL query parameters or decoded query strings), and non-constant
array literals evaluated as a result.

### Text forms (`value/text.rs`)

`TextForm<'a>` = `Borrowed(&'a str)` or `Inline { buf: [u8; 48], len: u8 }`. IPv6 max is
39 bytes, CIDR max 43, MAC max 23; URL text is precomputed and borrowed. So stringable
comparisons and `starts_with` never allocate. IP and CIDR text comes from std / ipnet
`Display` after mapped addresses are canonicalized, which matches the table in
`testdata/vectors/README.md`; MAC text is lowercase pairs.

### Parsing typed values

- IP / CIDR: std `IpAddr::from_str` plus a decimal prefix and `ipnet` truncation; 0
  differences from Go on ~2.4M differential inputs. Used by the lexer too.
- MAC: rulekit's own `mac.go` rule (6 or 8 bytes; `:`/`-` pairs or `.` groups of four).
- URL: `fluent-uri` (RFC 3986 URI-reference, no normalization); the text form is the input
  as written with scheme and host lowercased.

## 3. AST

D10/D11: one arena AST, private fields, public accessors.

- Go mutates `astPath.segments` in `parsePostfix`; the Rust parser builds paths in a local
  `Vec<Segment>` and pushes one arena node when the postfix loop ends, so arena ids stay
  dense and children precede parents.
- JSON (`ast/json.rs`): a hand `impl Serialize for Ast` that emits exactly `JSONAST`:
  `source`, `root` (ids `root`, `root.0`, …; `operator`/`negated`/`raw`/`path` with Go's
  `omitempty` semantics, `children` omitted when empty), `tokens` without EOF, spans with
  1-based line and byte column (port `lineIndex`). Vectors compare as JSON values, but key
  order follows Go's struct order. `Ast::json()` also exposes an owned JSON document.
- Tree-position ids ("root.0.1") are a JSON concern only; the API identifies nodes by
  `NodeId`. `NodeRef::tree_id()` returns the string form for tools that want it.

## 4. Compiled rules

`Rule` owns an `Arc<Ast>` and a compiled node tree. Lowering and node definitions
live in [src/eval/mod.rs](src/eval/mod.rs); the public API is documented in
[src/lib.rs](src/lib.rs).

- Lowering is a port of `lowerAST`, including: `in` with a CIDR literal → `Compare(EQ)`;
  negated binaries → `Not(base)` with one `Meta` for the pair (Go wraps once); literal
  value errors are reported as `ParseError` at the literal span.
- Literal regexes are compiled at compile time and live in `Literal(Value::Regex)`.
- `Meta.expr` is precomputed (Go also precomputes `tracedRule.expr`). Compiled nodes have
  no printer of their own: Go's per-node `String()` is only reachable through traces, which
  always use the AST text.
- Array operands evaluate into stack `SmallVec` buffers and compare in place.
  Items are evaluated in order, stopping at the first incomplete item. Arrays
  whose items are all literals are built once during compilation; a dynamic
  array used as a value materializes an owned `Value::Array`, as Go does.

## 5. Regex dialect (`src/regex/`)

Decision: no hand ports of Go library code. Regexes use `regex-syntax` +
`regex` with a dialect both Go and Rust accept identically; the Go side enforces
the same dialect in `checkRegexDialect`.

1. `literal_pattern`: rulekit's `parseRegex` extraction (pattern ends at the last delimiter,
   duplicate flags rejected, `(?flags)` prefix).
2. `check_regex_dialect`: rulekit's own dialect check from `regex.go`.
3. Parse with `regex_syntax::ast` and reject Rust-only or ambiguous syntax: inline flags
   other than `i m s U`, class set operations, nested classes, `\<` `\>` `\b{..}` and other
   special assertions, `\u`/`\U`, nested repetition, counts over 1000 or with leading
   zeros, Go's nested-count budget, more than 50 open groups, capture names outside
   `[A-Za-z_][A-Za-z0-9_]*`, and Unicode names outside the allow list.
4. Rewrite `\d \D \w \W \s \S` to Go's ASCII sets (`\s` = `[\t\n\f\r ]`), `\b` to
   `(?-u:\b)`, `\B` to `(?:(?-u:\B)(?:\b|\B))` (works around a `regex` 1.13 bug), and
   Unicode names to explicit `gc=`/`sc=` queries. `(?i)` folding of Perl classes matches Go.
5. `regex::RegexBuilder` with size limit 128 MiB.

Unicode property names: the allow list (`UNICODE_CLASSES`) holds the names Go 1.27.1 and
regex-syntax both accept with identical membership under `\p`, `\P`, and `(?i)`, with Go's
spelling rules. Membership follows each engine's Unicode version (Go: 17, regex-syntax: 16);
vectors stay off code points that differ.

## 6. Input, lazy values, Ctx

D2/D4/D9. Eval is generic over a caller context `C` (default `()`).

`Input` is object-safe and evaluation accepts unsized inputs, including
`dyn Input<C>`. The `Kv` map owns its entries; `KvInput` owns the map tree.
See [src/input.rs](src/input.rs) for the traits and lazy-entry API.

- Traversal ports `kvInput.GetPath` + `indexPath`: map key lookup, index into arrays,
  value fields on typed values (`fields.rs`), hand-off of the remaining path to a nested
  `Input`, lazy resolution only for map entries. Lazy results are `KvEntry<C>`, so a lazy
  may return a subtree containing more lazies or inputs, as in Go.
- `Lazy<C>` holds the shared context-aware closure, a `OnceLock` containing a
  boxed entry, and a mutex. Resolution: lock-free cell read → lock the entry
  mutex → re-check → call → store on success only. Resolution is single-flight
  per entry, errors are not memoized, and independent entries do not block each
  other. Plain values never touch a lock.
- Per-input-instance memo: Go keys the memo by path string inside each `kvInput`. Here the
  memo cell sits on the lazy entry itself and `KvInput` owns its tree (never a shared
  `Arc<Kv>`); cloning a `Lazy` produces a fresh, unresolved cell. A path string identifies
  exactly one map location, so this is the same memo, with no path-string hashing and no
  input-wide lock. Resolved values are borrowed from the cell for the input's lifetime.
- Go has two lazy types (`LazyValue`, `LazyContextValue`); Rust has one that receives
  `&C` (ignore it if unused).
- API difference from Go: lazies and nested inputs may only be map entries. Arrays inside a
  `Kv` hold plain `Value`s (Go also allows an `Input` as a `[]any` element).

## 7. Eval, results, options

- D8: `EnvBuilder::build` runs Go's `Opts.Validate` checks once (name conflicts with stdlib,
  macro vs function conflicts). `Env` cannot be built unvalidated, so eval never validates.
  `trace` is per call and lives in the cheap `Opts`, matching Go's `Opts.Trace` (D7).
- One lifetime `'a` covers rule, input, ctx and env so results can borrow from any of them.

Node results store the value and borrowed missing path names inline. A boxed
`Problem` holds rare errors or function-reported missing names, keeping the
happy path small. `missing_fields()` iterates path names and then
function-reported names. Result and error APIs are documented in
[src/eval/mod.rs](src/eval/mod.rs) and [src/error.rs](src/error.rs).

- Node semantics are a line-by-line port of `nodes.go` (And/Or short-circuit and
  ok/not-ok merging, Not, Match, Compare, In) and `compare*.go` (the full type matrix,
  `compareSliceDetailed` NE/CONTAINS rules, diagnostics). Numbers port `cmpNumber` incl.
  `cmp.Compare` NaN ordering and lossy int→float conversion.
- `is_zero` ports `isZero` (object/opaque/regex are truthy).

## 8. Tracing (D7)

Evaluation is generic over a trace slot: `()` without tracing, `Option<Frag>`
with tracing. `Rule::eval` branches once on `opts.trace`; the untraced
instantiation contains no trace code or per-node trace storage. Untraced
evaluation keeps the stack-buffer path for array operands; traced evaluation
evaluates them as nodes, as Go does, because traces need the array node.

Trace construction reproduces Go's shape: compiled nodes carry AST metadata,
except the base operator under `not in`/`not contains`/`not matches`, which Go
traces as one node. Unnamed `combineTrace` groups are fragments (`Frag`), not
boxed nodes. A traced node adopts its evaluation's children and diagnostics,
including an `and`/`or` that returns one side as-is adopting that side's
children. A macro call's only child is the expansion root.

`Trace<'a>` borrows expressions, values, and missing fields from the rule and
input; `into_owned()` detaches it. Its node id refers to the rule's AST or the
macro's AST for nodes inside an expansion, with kind stored separately. Trace
expressions are built at compile time in one bottom-up `canonical_all` pass.
See [src/eval/trace.rs](src/eval/trace.rs) for API details; shared trace vectors
exercise the shape, pruning, statuses, and diagnostics.

## 9. Functions and macros (D14, D15)

Decision: one typed function API, mirroring Go's `rulekit.Func`.
`Function::new::<A, R>` binds `Args` and `Returns`; derive support lives in
`rulekit-macros`. Signatures and examples are documented in
[src/func.rs](src/func.rs), not duplicated here.

- Each field of the derived struct is one positional argument, in order; the name is the
  field name (`#[rulekit(rename)]`). Field types implement `FromArg` (exact conversions;
  `TextForm` = string or text form; `ValueRef` = any). An optional last `Rest<'a>` field
  borrows the remaining arguments (variadic, no allocation). Enums, tuple structs, type
  parameters, more than one lifetime, and a misplaced `Rest` are compile errors
  (tests/ui).
- `Function::new::<A, R>` names the argument and return types (user decision) because a
  closure taking `HostArgs<'s>` and returning `&'a str` (borrowed from the context) needs a
  higher-ranked signature that Rust cannot infer from the closure alone; with A and R named,
  the closure needs no annotations. Return types give the declared type statically (`"any"` only for
  `ValueRef`/`Val`/`Value`). Results may borrow the context (`'a`) but not the arguments
  (`'s`), which live only for the call.
- `FuncSchema` is a plain struct of `&'static str` name and doc, so a struct literal works
  with string literals (run-time names can be leaked once at registration).
- `EnvBuilder::function(f)` takes the name from the schema; `build()` rejects duplicates,
  stdlib shadowing, and macro/function collisions.
- Functions get their arguments and the context only, never the rule's input. Macros take
  no parameters; parameterized logic is a function.
- Arity is checked before arguments are evaluated (`Error::ArgCount`, with `variadic` for
  a minimum); a wrongly typed argument is `Error::InvalidArg`; `FnError::Missing` makes the
  result unknown; `FnError::Error` becomes `Error::Function`.
- Arguments are evaluated into a stack `SmallVec<[Val; 4]>`: no map, no allocation.
- `starts_with` uses the same API (its `Args` impl is written out so the stdlib does not
  need the `derive` feature); parse-time arity errors for stdlib functions port
  `parseFunction`.

## 10. JSON input (`json_input.rs`)

`decode_json<C>(bytes, JsonOptions { annotated_keys, typed_document }) -> Result<Kv<C>, Error>`
keeps `json.go`'s semantics layer (annotated keys, typed documents, normalization). Parsing
uses `serde_json` with the additive `raw_value` feature so numbers keep their source text
(never `arbitrary_precision`): integers outside int64/uint64 are errors. Exactly one JSON
value (trailing non-whitespace is an error), at most 100 nesting levels counting the root,
typed `float64` strings decimal only. Base64 uses the `base64` crate's standard padded engine.

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
- One integration test runs every vector file, reporting every failing case name.

## 12. Benchmarks (`benches/eval.rs`, criterion)

Mirror the Go benchmarks one to one, same names, same expressions and inputs:

- `eval/<case>` for every `BenchmarkEval` case (short-circuit string, string slice, full
  traversal pass/no-match, nested path number, bracket path, array index, regex, ip_cidr,
  missing_fields, function, macro);
- `eval_lazy_input/{pruned,resolved_cached}`;
- `eval_trace`;
- `parse/{simple,complex}`;
- `cmp_number/<T>-<U>` over i64/u64/f64.
- Rust only: serde-shaped typed-event filters read through `derive(Input)`.

`tests/alloc.rs` uses a counting global allocator to assert zero allocations on
the happy path, including the shared evaluation/lazy cases from
`benches/cases.rs`, missing-field cases, and allocation-specific cases. Warm-up
is outside the measured evaluation. A materialized-array control checks that
the counter actually sees allocations.

`bench/compare.sh` runs Go and Rust benchmarks side by side.
`bench/size.sh` measures the traced-evaluation instantiation via the
`rulekit_size_probe` cfg. Benchmarks use fat LTO and one codegen unit.
