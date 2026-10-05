# Archived Go library ports

Hand ports of Go standard-library behavior, written during Phases 1–3 of the
Rust port and replaced in Phase 4 by crates and std (owner decision: no hand
ports of Go library code; shared behavior is defined by the vectors instead).
Kept for reference only: these files are not compiled and are not part of the
published package (`Cargo.toml` `include` covers `src/` only). The git branch
`archive/rust-go-ports` (48bc93b) has them wired in and building.

| File | What it was | Verification at the time |
|---|---|---|
| `regex/mod.rs` | Port of Go `parseRegex`, `checkRegexDialect`, and Go `regexp/syntax.Parse` (Perl flags), emitting a `regex`-crate pattern with every class written as explicit ranges from Go's Unicode tables and `SimpleFold`. | 0 accept/reject, message, or match differences vs go1.27.1 on all vector regexes, 447 patterns × 138 haystacks, 26 limit cases, 260k fuzzed patterns. Known gaps: `regex` compile-size limit on some Unicode-heavy patterns; alternation factoring near Go's depth/size limits. |
| `regex/unicode_names.rs` | Generated table of Go's `regexp/syntax` Unicode names, classes, fold tables, and `SimpleFold` (go1.27.1, Unicode 17.0.0). | Regenerated identically by `tools/check_unicode_names.sh`. |
| `tools/unicode_names/` | Standalone Go module that generated the table. | — |
| `tools/check_unicode_names.sh` | Regenerate-and-compare check for the table. | — |
| `ip.rs` | Port of Go `net.ParseIP`, `net.ParseCIDR`, `IPNet.Contains`, and the IP/CIDR text forms. | 0 differences vs Go on ~524k differential cases. |
| `url.rs` | Port of Go `net/url` `Parse`, `String`, `Hostname`, `Port`, `EscapedPath`, fragment/userinfo decoding (GODEBUG defaults of go1.27.1). | 0 differences vs Go on ~1.7M differential inputs. |
| `quote.rs` | Port of Go `strconv.Quote` (with `IsPrint` from the Go Unicode table) for bracket keys. | Unit tests against Go output. |
| `json_input.rs` | `decode_json` with a hand-written JSON reader mirroring Go `encoding/json` (`UseNumber` number text, last duplicate key wins, U+FFFD for invalid UTF-8, depth 10000, trailing data ignored), Go `base64.StdEncoding` decoding, and Go `ParseInt`/`ParseUint`/`ParseFloat` acceptance checks. | Unit tests and all vectors at the time. |
| `query.rs` | URL query lookup including a port of Go `strings.ToValidUTF8` (one U+FFFD per invalid run). | Unit tests. |
