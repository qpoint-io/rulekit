# Rulekit test vectors

Language-neutral test cases that every Rulekit implementation must pass. The Go
runner is `vectors_test.go` in the repository root.

## Files

Every `*.json` file in this directory is a vector file; any other entry except
this README is an error. Files group cases by area (`eval`, `literals`, `paths`,
`fields`, `regex`, `functions`, `macros`, `parse_errors`, `print`, `trace`,
`json_input`, `ast_json`, `text_forms`).

```json
{
  "input_mode": "annotated_keys",
  "cases": [
    {"name": "ip field in cidr", "expr": "ip in 192.168.0.0/16", "input": {"ip.$ip": "192.168.0.1"}, "expect": {"value": true}}
  ]
}
```

Decode files strictly: unknown keys anywhere in the file (outside `input`,
`value`, and `ast_json`, which hold arbitrary JSON) are errors, so a misspelled
expectation cannot silently pass. Case names are unique within a file.

## File keys

| Key | Meaning |
|---|---|
| `input_mode` | Default input mode for the file's cases (see `input`). |
| `cases` | Non-empty list of cases. |

## Case keys

| Key | Meaning |
|---|---|
| `name` | Required, unique within the file. |
| `expr` | The rule expression. Required unless `expect.decode_error` is set. |
| `input` | A JSON object decoded with the public JSON input decoder (`DecodeJSON` in Go) in the case's input mode. When absent, evaluate with no input at all. |
| `input_mode` | Overrides the file's mode: `plain`, `annotated_keys` (`"src.$ip": "1.2.3.4"`), or `typed_document` (`{"$type": "ip", "value": "1.2.3.4"}` everywhere). |
| `macros` | `{name: source}` macros registered for evaluation. |
| `expect` | Expectations; at least one key is required. |

## Expectations

Run the steps in order; each applies only when its keys are present.

1. **Decode.** `decode_error: true` — decoding `input` fails. Such a case has
   no `expr` and nothing else is checked. Error wording is not part of the
   contract.
2. **Parse.** `parse_error: true` — parsing `expr` fails; nothing else is
   checked. `parse_error: {"line": L, "column": C}` — also checks the 1-based
   error position. `parse_error: false` — parsing succeeds (use it alone for
   parse-only cases). Without `parse_error`, parsing must succeed.
3. **Print** (`print`). Each key compares one printer output:
   - `string`: the canonical expression (comments dropped).
   - `compact`: compact print (single line; comments kept as `/* */` unless a
     line comment contains `*/`).
   - `source`: the original expression text.
   - `multiline_2sp` / `multiline_4sp`: multiline print with a two- or
     four-space indent.

   Every `compact` and `multiline_*` output, and the `string` output, must
   parse again to the same canonical `string`. The Go runner also checks that
   printing the parsed AST and the compiled rule give the same output.
4. **AST JSON** (`ast_json`). The JSON AST document for `expr`, compared as
   JSON (key order and whitespace do not matter).
5. **Evaluate** when any of `value`, `error`, `missing_fields`, or `trace` is
   present. Evaluate twice, with tracing off and on; both results must match
   the expectations, and the untraced result has no trace.
   - `error`: `true` if evaluation must fail. Default `false`: no error. Error
     wording is not part of the contract.
   - `missing_fields`: the result's missing fields, compared as a sorted list.
     Default `[]`.
   - `value`: the result value (see below). Not checked when absent; `null`
     checks for no value.
   - `trace`: the trace tree (see below).

## Values

`value` uses the typed JSON input format with shorthand:

- `true`, `false`, `null`, and strings are themselves.
- Integers are `int64`, or `uint64` when they exceed the `int64` range; numbers
  with a fraction or exponent are `float64`.
- Arrays are arrays; objects without a `$type` key are objects.
- An object with a `$type` key is a typed JSON value exactly as in a
  `typed_document` input, e.g. `{"$type": "ip", "value": "10.0.0.1"}`,
  `{"$type": "bytes", "encoding": "hex", "value": "0a0b"}`,
  `{"$type": "uint64", "value": "18446744073709551615"}`. Its nested `object`
  and `array` members must be typed as well.

Values are equal when their types and contents match: `1` (`int64`) does not
equal `{"$type": "uint64", "value": "1"}`. Byte values compare by content
regardless of the encoding used to write them. Compare network values by their
text form (see below).

## Text forms

IP addresses, CIDRs, MAC addresses, and URLs compare with strings by a text
form, so the text form is part of the language. Implementations must produce
exactly these forms, whatever their platform libraries print:

| Type | Text form |
|---|---|
| IPv4 address | Dotted decimal without leading zeros: `10.0.0.1`. |
| IPv4-mapped IPv6 address (`::ffff:a.b.c.d`) | The IPv4 address in dotted decimal: `1.2.3.4`. |
| Other IPv6 address | RFC 5952: lowercase hex, leading zeros dropped in each group, the longest run of two or more zero groups replaced by `::` (the first run on a tie), and a single zero group not compressed. No embedded dotted form: `::1.2.3.4` prints as `::102:304`. |
| CIDR | The network address (host bits cleared) in the IP form above, `/`, and the prefix length in decimal. An IPv4-mapped network prints as IPv4 with the prefix reduced by 96: `::ffff:10.0.0.0/104` is `10.0.0.0/8`. |
| MAC address | Lowercase hex pairs separated by `:`: `aa:bb:cc:dd:ee:ff` (6 or 8 bytes). |
| URL | The URL as written, except the scheme and host are lowercase. Nothing is added, removed, or re-encoded: no default port is dropped, no `/` is added for an empty path, dot segments stay, an empty query (`http://h?`), an empty fragment (`http://h#`), and an empty authority (`http://`) are kept, and user info and percent escapes stay as written. |

`text_forms.json` pins each rule.

## Other shared rules

- **Regexes** use a dialect both Go's `regexp` and Rust's `regex` read the
  same way; `checkRegexDialect` (Go) and the Rust dialect check reject the
  rest. Unicode classes (`\pX`, `\p{...}`) accept only the 249 names both
  engines define identically (general categories and their long names,
  scripts, `Any`, `ASCII`, `Assigned`; not `LC`/`Cased_Letter`,
  `Cs`/`Surrogate`, or Unicode 17 scripts), spelled with Go's rules (case,
  spaces, `_`, and `-` ignored). Class membership and case folding follow
  each engine's Unicode version (Go 1.27: Unicode 17; Rust `regex`: Unicode
  16), so vectors avoid code points those versions disagree on. Compiled
  program size limits are implementation-defined: both reject very large
  programs, but not at the same point, so vectors stay well below them
  (at most one `\pL{1000}`).

- **URLs** (`$url` input) are RFC 3986 URI references written in ASCII, with
  no `%` escapes in the host, no IPvFuture literal (`[v1.x]`), no IPv6 zone,
  and at most one `@`; anything else is invalid input. Fields: `scheme` and
  `host` lowercase (IPv6 without brackets), `port` the digits after the host
  (missing when empty), `path` the RFC 3986 path as written (also for
  `mailto:a@b.com`, and `/p` for `///p`), `query` the raw query, `fragment`
  and `user` percent-decoded, or as written if the decoded bytes are not
  valid UTF-8.

- **Bracket keys** print as `"..."` with `"` and `\` escaped, `\n`, `\r`, and
  `\t` as escapes, other control characters (U+0000–U+001F, U+007F) as
  `\u00XX`, and every other character as is.
- **URL query values** are decoded like HTML forms (`&` separates pairs, `+` is
  a space, `%XX` is decoded, invalid escapes are kept). If the decoded bytes are
  not valid UTF-8, the value is the text as written.
- **JSON input** (`DecodeJSON`) must be exactly one JSON value; anything but
  whitespace after it is an error. Objects and arrays may nest at most 100
  levels, counting the root object. Integers outside the int64 and uint64
  ranges are errors, not floats. Typed `float64` strings must be decimal
  numbers (no hex floats, `inf`, or `nan`).

## Traces

`trace` describes the root of the trace tree. Every key is optional; only the
keys present are checked.

| Key | Meaning |
|---|---|
| `kind` | AST node kind: `literal`, `path`, `array`, `call`, `unary`, `binary`. |
| `expr` | The node's canonical expression. |
| `status` | `passed`, `failed`, `missing`, `error`, `pruned`, or `unknown`. |
| `value` | The node's value, as in `value` above. |
| `active` / `pruned` | Whether the node was evaluated / skipped by short-circuiting. |
| `missing_fields` | The node's missing fields, compared sorted. |
| `diagnostics` | The exact list of diagnostics, each `{code, left_type, operator, right_type}`. Types use typed JSON names (`int64`, `string`, `array`, ...). Each diagnostic must also carry a non-empty message; its wording is not part of the contract. |
| `children` | The exact list of child traces, each matched recursively. `{}` matches any child. |
