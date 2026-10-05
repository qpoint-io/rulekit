//! URL query parameters (port of `queryField`/`formDecode` in `fields.go`).

use std::borrow::Cow;

use super::{Val, Value, ValueRef};

/// Find a query parameter by name using the WHATWG
/// application/x-www-form-urlencoded rules: pairs are separated by `&`, the
/// first `=` splits name from value, `+` is a space, and percent escapes are
/// decoded (invalid escapes are kept as written). A single value is a string;
/// repeated values are a list. Decoding happens only for the requested key.
pub(crate) fn query_field<'a>(raw: &'a str, key: &str) -> Option<Val<'a>> {
    let mut first: Option<Cow<'a, str>> = None;
    let mut values: Vec<Value> = Vec::new();
    for pair in raw.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        if form_decode(name) != key {
            continue;
        }
        let value = form_decode(value);
        match &first {
            None => first = Some(value),
            Some(prev) => {
                if values.is_empty() {
                    values.push(Value::String(prev.clone().into_owned()));
                }
                values.push(Value::String(value.into_owned()));
            }
        }
    }
    if !values.is_empty() {
        return Some(Val::Owned(Value::Array(values)));
    }
    first.map(|value| match value {
        Cow::Borrowed(s) => Val::Ref(ValueRef::Str(s)),
        Cow::Owned(s) => Val::Owned(Value::String(s)),
    })
}

/// Decode `+` and percent escapes, keeping invalid escapes as written.
/// Invalid UTF-8 becomes U+FFFD, one per run of invalid bytes (Go
/// `strings.ToValidUTF8`).
fn form_decode(s: &str) -> Cow<'_, str> {
    if !s.contains(['+', '%']) {
        return Cow::Borrowed(s);
    }
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len()
                && b[i + 1].is_ascii_hexdigit()
                && b[i + 2].is_ascii_hexdigit() =>
            {
                out.push(hex(b[i + 1]) << 4 | hex(b[i + 2]));
                i += 2;
            }
            c => out.push(c),
        }
        i += 1;
    }
    Cow::Owned(to_valid_utf8(&out))
}

fn hex(c: u8) -> u8 {
    (c as char).to_digit(16).expect("hex digit") as u8
}

fn to_valid_utf8(mut b: &[u8]) -> String {
    let mut out = String::with_capacity(b.len());
    while !b.is_empty() {
        match std::str::from_utf8(b) {
            Ok(s) => {
                out.push_str(s);
                break;
            }
            Err(err) => {
                let valid = err.valid_up_to();
                out.push_str(std::str::from_utf8(&b[..valid]).expect("valid prefix"));
                out.push('\u{FFFD}');
                // Skip the whole run of invalid bytes.
                let mut rest = &b[valid..];
                loop {
                    match std::str::from_utf8(rest) {
                        Err(e) if e.valid_up_to() == 0 => {
                            let skip = e.error_len().unwrap_or(rest.len());
                            rest = &rest[skip..];
                            if rest.is_empty() {
                                break;
                            }
                        }
                        _ => break,
                    }
                }
                b = rest;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get(raw: &str, key: &str) -> Option<Value> {
        query_field(raw, key).map(Val::into_owned)
    }

    #[test]
    fn decodes_like_go() {
        assert_eq!(get("a=1;b=2", "a"), Some(Value::String("1;b=2".into())));
        assert_eq!(get("d=x+y", "d"), Some(Value::String("x y".into())));
        assert_eq!(get("c=%zz", "c"), Some(Value::String("%zz".into())));
        assert_eq!(get("a%20b=1", "a b"), Some(Value::String("1".into())));
        assert_eq!(get("flag", "flag"), Some(Value::String(String::new())));
        assert_eq!(get("&&a=1&", "a"), Some(Value::String("1".into())));
        assert_eq!(
            get("t=x&t=y&t=z", "t"),
            Some(Value::Array(vec![
                Value::String("x".into()),
                Value::String("y".into()),
                Value::String("z".into())
            ]))
        );
        assert_eq!(get("a=1", "b"), None);
        // %ff%fe is one invalid run: one replacement character.
        assert_eq!(
            get("v=%ff%fex", "v"),
            Some(Value::String("\u{FFFD}x".into()))
        );
        // Go: '%' needs two following bytes, so "%4" stays.
        assert_eq!(get("v=%4", "v"), Some(Value::String("%4".into())));
    }
}
