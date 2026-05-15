# Bracket Indexing for Field Paths

This note proposes extending the expression grammar with bracket indexing for field paths while preserving the existing use of square brackets for array literals.

## Motivation

Dot-separated field paths are convenient for common cases:

```text
request.headers.user_agent == "curl"
destination.ip in 192.168.0.0/16
```

They are less flexible when map keys contain dots, hyphens, spaces, slashes, reserved words, or other punctuation:

```text
labels["app.kubernetes.io/name"] == "api"
request.headers["user-agent"] == "curl"
payload["tls.version"] == "1.3"
```

Bracket indexing gives users an explicit escape hatch without weakening the identifier grammar.

## Proposed Syntax

Keep the existing array literal syntax:

```text
[1, "str", true]
field in [1, "str", 3]
```

Add bracket indexing as a postfix path operation:

```text
request.headers["user-agent"]
labels["app.kubernetes.io/name"]
items[0].name
```

Support root-level bracket paths for keys that cannot be written as identifiers:

```text
["top.level.key"] == 1
["weird key"] contains "value"
```

## Grammar Sketch

```text
primary       = literal | field_path | call | array | "(" expr ")"
array         = "[" [expr {"," expr}] "]"
field_path    = path_start {path_part}
path_start    = identifier | bracket_key
path_part     = "." identifier | bracket_key
bracket_key   = "[" (quoted_string | unsigned_integer) "]"
```

In a Pratt parser, `[` at the start of a primary remains an array literal. `[` after a field/path primary is parsed as postfix indexing.

## Parser Architecture

Use a Pike-style state-function lexer for tokenization, then feed those tokens into a Pratt parser for expression parsing.

```text
input string
  -> state-function lexer
  -> token stream
  -> Pratt parser
  -> AST
  -> semantic validation / compile step
  -> evaluator
```

The lexer should handle raw input concerns:

- whitespace
- `--` line comments
- `/* ... */` block comments
- quoted strings
- regex literals such as `/.../` and `|...|`
- operators and punctuation
- unquoted atoms

The Pratt parser should handle expression structure:

- operator precedence for `or`, `and`, comparisons, and prefix `not`
- grouping with parentheses
- array literals
- function and macro calls
- postfix bracket indexing

Prefer a pull-style lexer API such as `Next()` over a goroutine/channel lexer. The state-function pattern is still useful, but a pull API keeps parser control flow simple and avoids unnecessary channel overhead for short rule expressions.

## Migration Strategy

Rewrite the existing v1 feature set using the new lexer/parser/AST architecture before adding new language features.

This keeps parser architecture risk separate from language-design risk. The existing behavior should become the migration contract, and new syntax should only be added after the new implementation has strong parity with the current parser.

Suggested sequence:

1. Implement lexer parity for the current token set and accepted syntax.
2. Implement Pratt parser parity for existing expressions: literals, fields, arrays, calls, `not`, `and`, `or`, comparisons, `matches`, and `in`.
3. Build the explicit AST for the v1 grammar only.
4. Evaluate the AST directly or lower it to a current-style runtime representation.
5. Match current compact `String()` output where tests and public behavior depend on it.
6. Add parity tests that compare old and new parser behavior for parse success, compact output, eval result, missing-field behavior, and representative syntax errors.
7. Switch the existing `Parse()` API to the new parser once parity is strong.
8. Add v2 language/editor features after the replacement parser is stable.

Do not require exact yacc parse-error wording during the migration. Preserve useful line/column diagnostics and clear messages, but avoid coupling the new parser to generated-parser phrasing.

## AST and Round-Tripping

The AST should be designed as the shared model for both freeform text editing and a GUI expression editor. That means it should support both directions:

```text
expression -> lexer/parser -> AST
AST -> printer -> expression
```

The minimum invariant should be semantic round-tripping:

```text
Parse(Print(Parse(expr))) == Parse(expr)
```

Exact whitespace/comment preservation is a separate, stricter goal. If the editor needs to preserve comments and hand-formatted text, keep token trivia or a lightweight concrete syntax tree alongside the AST. If canonical formatting is acceptable after GUI edits, a normal AST plus a deterministic printer is enough.

AST nodes should carry:

- node kind
- source span for diagnostics and editor selection
- child nodes / operands
- normalized operator enum
- original operator spelling when useful for round-tripping or display
- literal value plus original raw token when useful

Prefer a small, explicit AST over evaluator-specific nodes. The evaluator can consume the AST directly by default. Performance-sensitive applications can add an optional second layer that compiles the AST into a runtime plan with precomputed lookups, compiled regexes, normalized operators, and other evaluation shortcuts. The editor should not have to reason about that optimized runtime representation.

## Printing and Formatting

Keep printing and formatting as explicit layers:

```text
Parse(expr) -> AST
AST.String() -> compact canonical expression
Format(AST, options) -> formatted expression
Rewrite(original, AST edits) -> preserve unchanged whitespace/comments
```

`String()` can continue to act as a compact canonical formatter for tests, logs, `EvaluatedRule`, snapshots, and debugging.

Add a formatter as an optional AST printer for editor and user-facing display:

- compact format: single-line, stable canonical expression
- multiline format: line-broken, indented expression for larger rules

The hybrid GUI/freeform editor should support three output modes:

- preserve: keep original whitespace/comments for unchanged source ranges and print only changed subtrees
- compact: canonical single-line output
- multiline: canonical formatted output with indentation

Formatting should be an explicit action or option. GUI edits should not automatically rewrite a user's whole expression unless that is the selected output mode.

For negated natural-language operators such as:

```text
field not contains "x"
domain not matches /example/
ip not in 10.0.0.0/8
```

represent them in the editable AST as a comparison with a negation flag or a distinct operator enum. A later semantic lowering step can convert them to `not (field contains "x")` if that is simpler for evaluation. This keeps GUI editing and expression printing straightforward.

## Initial Scope

Start with a deliberately small feature:

- quoted string keys for map/object access
- unsigned integer indexes for arrays/slices
- no dynamic expressions inside brackets

Do not initially support:

```text
headers[some_field]
headers[starts_with(name, "x")]
headers[1 + 2]
```

Dynamic indexing can be added later if there is a clear need, but the static form covers the main usability problem while keeping parsing, validation, and missing-field behavior straightforward.

## Semantics

Quoted bracket keys are exact map-key segments:

```text
obj["1"]
```

means the string key `"1"`, while:

```text
obj[1]
```

means numeric index `1` into an array or slice.

Existing dot-path behavior should remain compatible. If current lookup supports both flat keys like `"destination.ip"` and nested maps like `destination: {ip: ...}`, bracket indexing should act as the explicit form for exact path segments:

```text
destination.ip
destination["ip"]
["destination.ip"]
```

The last form should refer to the exact top-level key `"destination.ip"`.

## Reliability Notes

- Keep tokenization and semantic validation separate.
- Preserve source spans for useful syntax errors.
- Reject malformed brackets early: missing closing bracket, empty key, unsupported index expression.
- Add parser tests that distinguish array literals from postfix indexing.
- Add evaluation tests for flat keys, nested maps, string keys with dots, and numeric indexes.
