//! Literal grammar (port of the literal half of `parser.go` and `hexstring.go`).

use crate::ast::LiteralKind;
use crate::value::{Cidr, Ip, Value};

/// Parse an integer literal: optional sign, then decimal digits (leading zeros
/// are still decimal) or a `0x`/`0o`/`0b` prefix. Single underscores may
/// separate digits, and one may follow a base prefix. Values above the i64
/// range become u64. (Go `parseIntLiteral`.)
pub(crate) fn parse_int(s: &str) -> Option<Value> {
    let (negative, mut digits) = split_sign(s);
    let mut radix = 10;
    let bytes = digits.as_bytes();
    if bytes.len() > 2 && bytes[0] == b'0' {
        radix = match bytes[1] {
            b'x' | b'X' => 16,
            b'o' | b'O' => 8,
            b'b' | b'B' => 2,
            _ => 10,
        };
        if radix != 10 {
            digits = &digits[2..];
            digits = digits.strip_prefix('_').unwrap_or(digits);
        }
    }
    if digits.is_empty()
        || digits.starts_with('_')
        || digits.ends_with('_')
        || digits.contains("__")
    {
        return None;
    }
    let digits = digits.replace('_', "");
    // from_str_radix accepts a leading sign itself; Go's ParseInt/ParseUint
    // were given the sign we split off, so reject a second one here.
    if !digits.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return None;
    }
    let magnitude = u64::from_str_radix(&digits, radix).ok()?;
    if negative {
        // i64::MIN has magnitude 2^63.
        if magnitude <= i64::MAX as u64 + 1 {
            return Some(Value::Int((magnitude as i64).wrapping_neg()));
        }
        return None;
    }
    match i64::try_from(magnitude) {
        Ok(n) => Some(Value::Int(n)),
        Err(_) => Some(Value::Uint(magnitude)),
    }
}

/// Whether `s` is a decimal float: optional sign, digits, then a fraction
/// (`5.`, `1.5`), an exponent (`1e3`), or both. (Go `isFloat`.)
pub(crate) fn is_float(s: &str) -> bool {
    let (_, s) = split_sign(s);
    let b = s.as_bytes();
    let mut i = skip_digits(b, 0);
    if i == 0 {
        return false;
    }
    let mut fraction = false;
    let mut exponent = false;
    if i < b.len() && b[i] == b'.' {
        fraction = true;
        i = skip_digits(b, i + 1);
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let start = i;
        i = skip_digits(b, i);
        if i == start {
            return false;
        }
        exponent = true;
    }
    i == b.len() && (fraction || exponent)
}

/// Go `strconv.ParseFloat(s, 64)` for a string accepted by [`is_float`]:
/// overflow to infinity is an error, underflow is not.
fn parse_float(s: &str) -> Result<f64, String> {
    let value: f64 = s.parse().map_err(|err| format!("{err}"))?;
    if value.is_infinite() {
        return Err("value out of range".to_owned());
    }
    Ok(value)
}

fn split_sign(s: &str) -> (bool, &str) {
    match s.as_bytes().first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    }
}

fn skip_digits(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    i
}

/// Decode a quoted literal. Backticks preserve their contents exactly; single
/// and double quotes accept Go `strconv.Unquote` escapes, plus \' and \".
/// The result must be valid UTF-8. (Go `unquote`.)
pub(crate) fn unquote(raw: &str) -> Result<String, String> {
    if raw.starts_with('`') {
        return Ok(raw[1..raw.len() - 1].to_owned());
    }
    let inner = &raw.as_bytes()[1..raw.len() - 1];
    let mut out: Vec<u8> = Vec::with_capacity(inner.len());
    let mut i = 0;
    while i < inner.len() {
        let c = inner[i];
        if c == b'\n' {
            return Err("invalid syntax".to_owned());
        }
        if c != b'\\' {
            out.push(c);
            i += 1;
            continue;
        }
        if i + 1 >= inner.len() {
            // Go appends the lone backslash, which then escapes the closing
            // quote: an unterminated string.
            return Err("invalid syntax".to_owned());
        }
        let esc = inner[i + 1];
        i += 2;
        let simple = match esc {
            b'a' => Some(0x07),
            b'b' => Some(0x08),
            b'f' => Some(0x0c),
            b'n' => Some(b'\n'),
            b'r' => Some(b'\r'),
            b't' => Some(b'\t'),
            b'v' => Some(0x0b),
            b'\\' => Some(b'\\'),
            b'\'' | b'"' => Some(esc),
            _ => None,
        };
        if let Some(byte) = simple {
            out.push(byte);
            continue;
        }
        match esc {
            b'x' | b'u' | b'U' => {
                let n = match esc {
                    b'x' => 2,
                    b'u' => 4,
                    _ => 8,
                };
                let digits = inner.get(i..i + n).ok_or("invalid syntax")?;
                let mut v: u32 = 0;
                for &d in digits {
                    let x = (d as char).to_digit(16).ok_or("invalid syntax")?;
                    v = v << 4 | x;
                }
                i += n;
                if esc == b'x' {
                    out.push(v as u8);
                } else {
                    let c = char::from_u32(v).ok_or("invalid syntax")?;
                    let mut buf = [0; 4];
                    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                }
            }
            b'0'..=b'7' => {
                let mut v = u32::from(esc - b'0');
                let digits = inner.get(i..i + 2).ok_or("invalid syntax")?;
                for &d in digits {
                    if !(b'0'..=b'7').contains(&d) {
                        return Err("invalid syntax".to_owned());
                    }
                    v = v << 3 | u32::from(d - b'0');
                }
                i += 2;
                if v > 255 {
                    return Err("invalid syntax".to_owned());
                }
                out.push(v as u8);
            }
            _ => return Err("invalid syntax".to_owned()),
        }
    }
    String::from_utf8(out)
        .map_err(|_| "string is not valid UTF-8; use x\"...\" for bytes".to_owned())
}

/// Parse colon-separated hex pairs (`50:4f`) or an `x"..."` literal whose
/// digits may be separated by colons. (Go `ParseHexString`.)
pub(crate) fn parse_hex(raw: &str) -> Result<Vec<u8>, String> {
    let b = raw.as_bytes();
    let mut digits = raw;
    if b.len() >= 3
        && matches!(b[0], b'x' | b'X')
        && matches!(b[1], b'"' | b'\'')
        && b[b.len() - 1] == b[1]
    {
        digits = &raw[2..raw.len() - 1];
        if digits.is_empty() {
            return Err("empty hex literal".to_owned());
        }
    }
    let digits: Vec<u8> = digits.bytes().filter(|&c| c != b':').collect();
    if !digits.len().is_multiple_of(2) {
        return Err("odd length hex string".to_owned());
    }
    digits
        .chunks_exact(2)
        .map(|pair| {
            let hi = (pair[0] as char).to_digit(16);
            let lo = (pair[1] as char).to_digit(16);
            match (hi, lo) {
                (Some(hi), Some(lo)) => Ok((hi << 4 | lo) as u8),
                _ => Err("invalid byte in hex string".to_owned()),
            }
        })
        .collect()
}

/// Parse a literal token into its value (Go `parseValueToken`). Errors are the
/// message of a Go `ValueParseError`.
pub(crate) fn parse_literal(kind: LiteralKind, raw: &str) -> Result<Value, String> {
    let result = match kind {
        LiteralKind::String => unquote(raw).map(Value::String),
        LiteralKind::Int => {
            parse_int(raw).ok_or_else(|| format!("parsing integer: invalid value {raw:?}"))
        }
        LiteralKind::Float => parse_float(raw).map(Value::Float),
        LiteralKind::Bool => Ok(Value::Bool(raw.eq_ignore_ascii_case("true"))),
        LiteralKind::Ip => Ip::parse(raw).map(Value::Ip).map_err(|e| e.to_string()),
        LiteralKind::Cidr => Cidr::parse(raw).map(Value::Cidr).map_err(|e| e.to_string()),
        LiteralKind::HexString => parse_hex(raw).map(Value::Bytes),
        LiteralKind::Regex => {
            crate::regex::compile_literal(raw).map(|re| Value::Regex(Box::new(re)))
        }
    };
    result.map_err(|err| format!("parsing {} value {raw:?}: {err}", kind_name(kind)))
}

fn kind_name(kind: LiteralKind) -> &'static str {
    match kind {
        LiteralKind::String => "string",
        LiteralKind::Int => "integer",
        LiteralKind::Float => "float",
        LiteralKind::Bool => "boolean",
        LiteralKind::Ip => "IP",
        LiteralKind::Cidr => "CIDR",
        LiteralKind::HexString => "hex string",
        LiteralKind::Regex => "regex",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ints() {
        assert_eq!(parse_int("010"), Some(Value::Int(10)));
        assert_eq!(parse_int("-010"), Some(Value::Int(-10)));
        assert_eq!(parse_int("+010"), Some(Value::Int(10)));
        assert_eq!(parse_int("0x_1f"), Some(Value::Int(31)));
        assert_eq!(parse_int("0b101"), Some(Value::Int(5)));
        assert_eq!(parse_int("0o17"), Some(Value::Int(15)));
        assert_eq!(parse_int("00"), Some(Value::Int(0)));
        assert_eq!(parse_int("0x"), None);
        assert_eq!(parse_int("1__0"), None);
        assert_eq!(parse_int("1_"), None);
        assert_eq!(parse_int("--1"), None);
        assert_eq!(parse_int("-+1"), None);
        assert_eq!(
            parse_int("18446744073709551615"),
            Some(Value::Uint(u64::MAX))
        );
        assert_eq!(parse_int("18446744073709551616"), None);
        assert_eq!(
            parse_int("-9223372036854775808"),
            Some(Value::Int(i64::MIN))
        );
        assert_eq!(parse_int("-9223372036854775809"), None);
    }

    #[test]
    fn floats() {
        assert!(is_float("5."));
        assert!(is_float("1e3"));
        assert!(is_float("-1.5e-3"));
        assert!(!is_float(".5"));
        assert!(!is_float("1_0.5"));
        assert!(!is_float("0x1.8p1"));
        assert!(parse_float("1e400").is_err());
        assert_eq!(parse_float("1e-400"), Ok(0.0));
    }

    #[test]
    fn strings() {
        assert_eq!(unquote(r#""te\"x't""#).unwrap(), "te\"x't");
        assert_eq!(unquote(r#"'te"s\'t'"#).unwrap(), "te\"s't");
        assert_eq!(unquote(r#""it\'s""#).unwrap(), "it's");
        assert_eq!(unquote(r#""\u00e9""#).unwrap(), "é");
        assert!(unquote(r#""\xff""#).is_err());
        assert!(unquote(r"'\377'").is_err());
        assert!(unquote(r#""\400""#).is_err());
        assert!(unquote(r#""\ud800""#).is_err());
        assert!(unquote("\"a\nb\"").is_err());
        assert!(unquote(r#""\q""#).is_err());
        assert_eq!(unquote(r#""\x41\101""#).unwrap(), "AA");
    }

    #[test]
    fn hex() {
        assert_eq!(parse_hex("x\"0a\"").unwrap(), vec![0x0a]);
        assert_eq!(parse_hex("X'50:4F:53:54'").unwrap(), b"POST".to_vec());
        assert_eq!(parse_hex("50:4f").unwrap(), b"PO".to_vec());
        assert!(parse_hex("x\"\"").is_err());
        assert!(parse_hex("x\"abc\"").is_err());
        assert!(parse_hex("x\"0g\"").is_err());
    }
}
