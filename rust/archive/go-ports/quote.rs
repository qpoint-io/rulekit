//! Port of Go `strconv.Quote` for valid UTF-8 input.

use std::fmt::Write;

use crate::regex::in_go_class;

/// Append `s` as a Go double-quoted string literal.
pub(crate) fn quote_into(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            _ if is_print(c) => out.push(c),
            '\x07' => out.push_str("\\a"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x0b' => out.push_str("\\v"),
            _ if c < ' ' || c == '\x7f' => {
                let _ = write!(out, "\\x{:02x}", c as u32);
            }
            _ if (c as u32) < 0x10000 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            _ => {
                let _ = write!(out, "\\U{:08x}", c as u32);
            }
        }
    }
    out.push('"');
}

/// Go `strconv.IsPrint`: letters, marks, numbers, punctuation, symbols, and
/// the ASCII space.
fn is_print(c: char) -> bool {
    if c.is_ascii() {
        return (' '..='~').contains(&c);
    }
    ["L", "M", "N", "P", "S"]
        .iter()
        .any(|class| in_go_class(class, c))
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
    fn quotes_like_go() {
        assert_eq!(quote("user-agent"), r#""user-agent""#);
        assert_eq!(quote("it\"s\\"), r#""it\"s\\""#);
        assert_eq!(quote("a\tb\x01\x7f"), r#""a\tb\x01\x7f""#);
        assert_eq!(quote("ñame é"), "\"ñame é\"");
        assert_eq!(quote("\u{a0}\u{ad}"), r#""\u00a0\u00ad""#);
        assert_eq!(quote("\u{e0000}"), r#""\U000e0000""#);
    }
}
