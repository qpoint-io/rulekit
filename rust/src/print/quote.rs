//! Quoting bracket keys (the shared rule in testdata/vectors/README.md,
//! "Other shared rules").

use std::fmt::Write;

/// Append `key` quoted: `"` and `\` escaped; `\n`, `\r`, `\t` as escapes;
/// other control characters (U+0000–U+001F, U+007F) as `\u00XX`; every other
/// character as is.
pub(crate) fn quote_into(out: &mut String, key: &str) {
    out.push('"');
    for c in key.chars() {
        match c {
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0'..='\x1f' | '\x7f' => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            _ => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quote(s: &str) -> String {
        let mut out = String::new();
        quote_into(&mut out, s);
        out
    }

    #[test]
    fn quotes_keys() {
        assert_eq!(quote("user-agent"), r#""user-agent""#);
        assert_eq!(quote("it\"s\\"), r#""it\"s\\""#);
        assert_eq!(quote("a\tb\n\r\x01\x7f"), r#""a\tb\n\r\u0001\u007f""#);
        assert_eq!(quote("ñame é \u{a0}\u{ad}\u{e0000}"), "\"ñame é \u{a0}\u{ad}\u{e0000}\"");
    }
}
