//! JSON input decoding (port of `json.go`).
//!
//! The JSON reader is hand-written to follow Go's `encoding/json` decoder
//! exactly where it matters here: numbers keep their source text (Go
//! `UseNumber`), duplicate keys keep the last value, invalid UTF-8 and lone
//! surrogates become U+FFFD, nesting is limited to 10000 levels, and data
//! after the first value is ignored (`Decoder.Decode`).

use crate::error::Error;
use crate::input::{Kv, KvEntry};
use crate::value::{Cidr, Ip, Mac, Map, Url, Value};

/// How [`decode_json`] reads values.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct JsonOptions {
    /// Enable suffix type hints such as `"src.$ip": "1.2.3.4"`.
    pub annotated_keys: bool,
    /// Require every field to be a typed object such as
    /// `{"$type": "ip", "value": "1.2.3.4"}`. Exclusive with `annotated_keys`.
    pub typed_document: bool,
}

/// Decode a JSON object into a [`Kv`] (Go `DecodeJSON`).
pub fn decode_json<C: ?Sized>(data: &[u8], opts: JsonOptions) -> Result<Kv<C>, Error> {
    if opts.annotated_keys && opts.typed_document {
        return Err(err(
            "json options annotated_keys and typed_document are mutually exclusive",
        ));
    }
    let raw = Reader {
        b: data,
        pos: 0,
        depth: 0,
    }
    .document()?;
    let map = if opts.typed_document {
        typed_document(raw)?
    } else {
        match plain(raw, opts.annotated_keys)? {
            Value::Object(map) => map,
            _ => return Err(err("json root must be an object")),
        }
    };
    Ok(map
        .into_iter()
        .map(|(k, v)| (k, KvEntry::Value(v)))
        .collect())
}

fn err(msg: impl Into<String>) -> Error {
    Error::Json(msg.into())
}

fn ctx(prefix: String, e: Error) -> Error {
    match e {
        Error::Json(msg) => Error::Json(format!("{prefix}: {msg}")),
        other => other,
    }
}

/// A parsed JSON value; numbers keep their text.
enum Json {
    Null,
    Bool(bool),
    Number(String),
    String(String),
    Array(Vec<Json>),
    Object(Map<Json>),
}

impl Json {
    fn type_name(&self) -> &'static str {
        match self {
            Json::Null => "<nil>",
            Json::Bool(_) => "bool",
            Json::Number(_) => "json.Number",
            Json::String(_) => "string",
            Json::Array(_) => "[]interface {}",
            Json::Object(_) => "map[string]interface {}",
        }
    }
}

const MAX_DEPTH: usize = 10000;

struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
    depth: usize,
}

impl Reader<'_> {
    fn document(mut self) -> Result<Json, Error> {
        self.ws();
        if self.pos >= self.b.len() {
            return Err(err("unexpected end of JSON input"));
        }
        self.value()
    }

    fn ws(&mut self) {
        while self.pos < self.b.len() && matches!(self.b[self.pos], b' ' | b'\t' | b'\n' | b'\r') {
            self.pos += 1;
        }
    }

    fn fail<T>(&self, what: &str) -> Result<T, Error> {
        Err(err(format!("invalid JSON at offset {}: {what}", self.pos)))
    }

    fn peek(&self) -> Option<u8> {
        self.b.get(self.pos).copied()
    }

    fn value(&mut self) -> Result<Json, Error> {
        self.ws();
        match self.peek() {
            None => self.fail("unexpected end of input"),
            Some(b'{') => self.nested(Self::object),
            Some(b'[') => self.nested(Self::array),
            Some(b'"') => self.string().map(Json::String),
            Some(b't') => self.literal(b"true", Json::Bool(true)),
            Some(b'f') => self.literal(b"false", Json::Bool(false)),
            Some(b'n') => self.literal(b"null", Json::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => self.fail("unexpected character"),
        }
    }

    fn nested(&mut self, f: fn(&mut Self) -> Result<Json, Error>) -> Result<Json, Error> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return self.fail("exceeded max depth");
        }
        let out = f(self);
        self.depth -= 1;
        out
    }

    fn literal(&mut self, word: &[u8], value: Json) -> Result<Json, Error> {
        if self.b[self.pos..].starts_with(word) {
            self.pos += word.len();
            Ok(value)
        } else {
            self.fail("invalid literal")
        }
    }

    fn object(&mut self) -> Result<Json, Error> {
        self.pos += 1;
        let mut map = Map::default();
        self.ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Json::Object(map));
        }
        loop {
            self.ws();
            if self.peek() != Some(b'"') {
                return self.fail("expected object key");
            }
            let key = self.string()?;
            self.ws();
            if self.peek() != Some(b':') {
                return self.fail("expected ':'");
            }
            self.pos += 1;
            let value = self.value()?;
            map.insert(key, value);
            self.ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Json::Object(map));
                }
                _ => return self.fail("expected ',' or '}'"),
            }
        }
    }

    fn array(&mut self) -> Result<Json, Error> {
        self.pos += 1;
        let mut items = Vec::new();
        self.ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Json::Array(items));
        }
        loop {
            items.push(self.value()?);
            self.ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Json::Array(items));
                }
                _ => return self.fail("expected ',' or ']'"),
            }
        }
    }

    fn number(&mut self) -> Result<Json, Error> {
        let start = self.pos;
        let digits = |r: &mut Self| {
            let s = r.pos;
            while r.peek().is_some_and(|c| c.is_ascii_digit()) {
                r.pos += 1;
            }
            r.pos > s
        };
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        match self.peek() {
            Some(b'0') => self.pos += 1,
            Some(b'1'..=b'9') => {
                digits(self);
            }
            _ => return self.fail("invalid number"),
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            if !digits(self) {
                return self.fail("invalid number");
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if !digits(self) {
                return self.fail("invalid number");
            }
        }
        let text = std::str::from_utf8(&self.b[start..self.pos]).expect("ASCII number");
        Ok(Json::Number(text.to_owned()))
    }

    /// A string literal; `self.pos` is on the opening quote.
    fn string(&mut self) -> Result<String, Error> {
        self.pos += 1;
        let mut out = String::new();
        loop {
            let Some(c) = self.peek() else {
                return self.fail("unterminated string");
            };
            match c {
                b'"' => {
                    self.pos += 1;
                    return Ok(out);
                }
                b'\\' => {
                    self.pos += 1;
                    let Some(e) = self.peek() else {
                        return self.fail("unterminated string");
                    };
                    self.pos += 1;
                    match e {
                        b'"' | b'\\' | b'/' => out.push(e as char),
                        b'b' => out.push('\x08'),
                        b'f' => out.push('\x0c'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hi = self.hex4()?;
                            out.push(self.surrogate(hi));
                        }
                        _ => return self.fail("invalid escape"),
                    }
                }
                0..=0x1f => return self.fail("control character in string"),
                _ if c.is_ascii() => {
                    out.push(c as char);
                    self.pos += 1;
                }
                _ => {
                    // Decode one UTF-8 sequence; each invalid byte is U+FFFD.
                    let rest = &self.b[self.pos..self.b.len().min(self.pos + 4)];
                    let valid = match std::str::from_utf8(rest) {
                        Ok(s) => s,
                        Err(e) => {
                            std::str::from_utf8(&rest[..e.valid_up_to()]).expect("valid prefix")
                        }
                    };
                    match valid.chars().next() {
                        Some(ch) => {
                            out.push(ch);
                            self.pos += ch.len_utf8();
                        }
                        None => {
                            out.push('\u{FFFD}');
                            self.pos += 1;
                        }
                    }
                }
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, Error> {
        let Some(digits) = self.b.get(self.pos..self.pos + 4) else {
            return self.fail("invalid \\u escape");
        };
        let mut v = 0;
        for &d in digits {
            let Some(x) = (d as char).to_digit(16) else {
                return self.fail("invalid \\u escape");
            };
            v = v << 4 | x;
        }
        self.pos += 4;
        Ok(v)
    }

    /// Combine a `\u` escape with a following low surrogate, as Go does; a
    /// lone surrogate is U+FFFD.
    fn surrogate(&mut self, hi: u32) -> char {
        if (0xD800..0xDC00).contains(&hi) && self.b[self.pos..].starts_with(b"\\u") {
            let save = self.pos;
            self.pos += 2;
            if let Ok(lo) = self.hex4()
                && (0xDC00..0xE000).contains(&lo)
            {
                return char::from_u32(0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00))
                    .expect("valid pair");
            }
            self.pos = save;
            return '\u{FFFD}';
        }
        char::from_u32(hi).unwrap_or('\u{FFFD}')
    }
}

/// Go `normalizePlainJSONValue`.
fn plain(value: Json, annotated: bool) -> Result<Value, Error> {
    Ok(match value {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Bool(b),
        Json::String(s) => Value::String(s),
        Json::Number(n) => number(&n)?,
        Json::Array(items) => Value::Array(
            items
                .into_iter()
                .enumerate()
                .map(|(i, v)| plain(v, annotated).map_err(|e| ctx(format!("index {i}"), e)))
                .collect::<Result<_, _>>()?,
        ),
        Json::Object(map) => {
            let mut out = Map::default();
            for (key, child) in map {
                let (out_key, suffix) = if annotated {
                    split_annotated(&key)
                } else {
                    (key.as_str(), None)
                };
                if out.contains_key(out_key) {
                    return Err(err(format!("duplicate normalized key {out_key:?}")));
                }
                let mut value =
                    plain(child, annotated).map_err(|e| ctx(format!("key {key:?}"), e))?;
                if let Some(suffix) = suffix {
                    value = scalar(suffix, Raw::Value(&value), "")
                        .map_err(|e| ctx(format!("key {key:?}"), e))?;
                }
                out.insert(out_key.to_owned(), value);
            }
            Value::Object(out)
        }
    })
}

/// Go `normalizeJSONNumber`.
fn number(n: &str) -> Result<Value, Error> {
    if n.contains(['.', 'e', 'E']) {
        return parse_float(n).map(Value::Float);
    }
    if let Ok(i) = n.parse::<i64>() {
        return Ok(Value::Int(i));
    }
    if let Some(u) = parse_uint(n) {
        return Ok(Value::Uint(u));
    }
    Err(err(format!("invalid json number {n:?}")))
}

const SUFFIXES: [&str; 13] = [
    ".$bytes_base64",
    ".$bytes_hex",
    ".$base64",
    ".$hex",
    ".$float64",
    ".$uint64",
    ".$int64",
    ".$bool",
    ".$string",
    ".$cidr",
    ".$mac",
    ".$url",
    ".$ip",
];

/// Split `name.$type` into `name` and `type` (Go `splitAnnotatedKey`).
fn split_annotated(key: &str) -> (&str, Option<&str>) {
    for suffix in SUFFIXES {
        if let Some(name) = key.strip_suffix(suffix) {
            return (name, Some(&suffix[2..]));
        }
    }
    (key, None)
}

/// Go `normalizeTypedJSONDocument`.
fn typed_document(raw: Json) -> Result<Map<Value>, Error> {
    let Json::Object(root) = raw else {
        return Err(err("typed json root must be an object"));
    };
    let mut out = Map::default();
    for (key, child) in root {
        if let (_, Some(suffix)) = split_annotated(&key) {
            return Err(err(format!(
                "typed json key {key:?} must not use annotated suffix \".${suffix}\""
            )));
        }
        let value = typed_node(child).map_err(|e| ctx(format!("key {key:?}"), e))?;
        out.insert(key, value);
    }
    Ok(out)
}

/// Go `decodeTypedJSONNode`.
fn typed_node(value: Json) -> Result<Value, Error> {
    let Json::Object(mut object) = value else {
        return Err(err("typed json value must be an object"));
    };
    let typ = match object.get("$type") {
        Some(Json::String(t)) if !t.is_empty() => t.clone(),
        _ => return Err(err("typed json value requires $type")),
    };
    let Some(value) = object.remove("value") else {
        return Err(err("typed json value requires value"));
    };
    match typ.as_str() {
        "object" => {
            let Json::Object(map) = value else {
                return Err(err(format!("expected object, got {}", value.type_name())));
            };
            let mut out = Map::default();
            for (key, child) in map {
                out.insert(
                    key.clone(),
                    typed_node(child).map_err(|e| ctx(format!("key {key:?}"), e))?,
                );
            }
            Ok(Value::Object(out))
        }
        "array" => {
            let Json::Array(items) = value else {
                return Err(err(format!("expected array, got {}", value.type_name())));
            };
            Ok(Value::Array(
                items
                    .into_iter()
                    .enumerate()
                    .map(|(i, v)| typed_node(v).map_err(|e| ctx(format!("index {i}"), e)))
                    .collect::<Result<_, _>>()?,
            ))
        }
        _ => {
            let encoding = match object.get("encoding") {
                Some(Json::String(e)) => e.as_str(),
                _ => "",
            };
            scalar(&typ, Raw::Json(&value), encoding)
        }
    }
}

/// A scalar input: raw JSON (typed documents) or an already-normalized value
/// (annotated keys).
#[derive(Clone, Copy)]
enum Raw<'a> {
    Json(&'a Json),
    Value(&'a Value),
}

impl<'a> Raw<'a> {
    fn type_name(self) -> &'static str {
        match self {
            Raw::Json(j) => j.type_name(),
            Raw::Value(v) => match v {
                Value::Null => "<nil>",
                Value::Bool(_) => "bool",
                Value::Int(_) => "int64",
                Value::Uint(_) => "uint64",
                Value::Float(_) => "float64",
                Value::String(_) => "string",
                Value::Array(_) => "[]interface {}",
                Value::Object(_) => "map[string]interface {}",
                _ => "unknown",
            },
        }
    }

    fn string(self) -> Result<&'a str, Error> {
        match self {
            Raw::Json(Json::String(s)) => Ok(s),
            Raw::Value(Value::String(s)) => Ok(s),
            _ => Err(err(format!("expected string, got {}", self.type_name()))),
        }
    }

    /// Go `numericString`.
    fn numeric(self) -> Result<String, Error> {
        match self {
            Raw::Json(Json::Number(n)) => Ok(n.clone()),
            Raw::Json(Json::String(s)) | Raw::Value(Value::String(s)) => Ok(s.clone()),
            Raw::Value(Value::Int(n)) => Ok(n.to_string()),
            Raw::Value(Value::Uint(n)) => Ok(n.to_string()),
            // Go strconv.FormatFloat(v, 'f', -1, 64): shortest digits, no exponent.
            Raw::Value(Value::Float(n)) => Ok(n.to_string()),
            _ => Err(err(format!(
                "expected number or string, got {}",
                self.type_name()
            ))),
        }
    }
}

/// Go `decodeScalarValue`.
fn scalar(typ: &str, value: Raw<'_>, encoding: &str) -> Result<Value, Error> {
    Ok(match typ {
        "ip" => {
            let s = value.string()?;
            Value::Ip(Ip::parse(s).ok_or_else(|| err(format!("invalid ip {s:?}")))?)
        }
        "cidr" => {
            let s = value.string()?;
            Value::Cidr(Cidr::parse(s).ok_or_else(|| err(format!("invalid CIDR address: {s}")))?)
        }
        "mac" => {
            let s = value.string()?;
            Value::Mac(Mac::parse(s).ok_or_else(|| err(format!("invalid mac {s:?}")))?)
        }
        "url" => Value::Url(Box::new(Url::parse(value.string()?).map_err(err)?)),
        "bytes" => bytes(value, encoding)?,
        "hex" | "bytes_hex" => bytes(value, "hex")?,
        "base64" | "bytes_base64" => bytes(value, "base64")?,
        "string" => Value::String(value.string()?.to_owned()),
        "bool" => match value {
            Raw::Json(Json::Bool(b)) | Raw::Value(Value::Bool(b)) => Value::Bool(*b),
            _ => return Err(err(format!("expected bool, got {}", value.type_name()))),
        },
        "null" => match value {
            Raw::Json(Json::Null) | Raw::Value(Value::Null) => Value::Null,
            _ => return Err(err(format!("expected null, got {}", value.type_name()))),
        },
        "int64" => {
            let s = value.numeric()?;
            Value::Int(parse_int(&s).ok_or_else(|| err(format!("invalid int64 {s:?}")))?)
        }
        "uint64" => {
            let s = value.numeric()?;
            Value::Uint(parse_uint(&s).ok_or_else(|| err(format!("invalid uint64 {s:?}")))?)
        }
        "float64" => Value::Float(parse_float(&value.numeric()?)?),
        _ => return Err(err(format!("unknown type {typ:?}"))),
    })
}

fn bytes(value: Raw<'_>, encoding: &str) -> Result<Value, Error> {
    let s = value.string()?;
    let decoded = match encoding {
        "hex" => decode_hex(&s.replace(':', "")),
        "base64" => decode_base64(s),
        _ => return Err(err("bytes encoding must be hex or base64")),
    };
    decoded
        .map(Value::Bytes)
        .ok_or_else(|| err(format!("invalid {encoding} bytes")))
}

/// Go `strconv.ParseInt(s, 10, 64)`.
fn parse_int(s: &str) -> Option<i64> {
    let digits = s.strip_prefix(['+', '-']).unwrap_or(s);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// Go `strconv.ParseUint(s, 10, 64)`: no sign.
fn parse_uint(s: &str) -> Option<u64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// Go `strconv.ParseFloat(s, 64)` for decimal input: `inf`/`infinity`/`nan`
/// (any case, optional sign) are accepted; finite text that overflows is an
/// error. Hexadecimal floats and `_` separators are not supported.
fn parse_float(s: &str) -> Result<f64, Error> {
    let fail = || err(format!("invalid float64 {s:?}"));
    let unsigned = s.strip_prefix(['+', '-']).unwrap_or(s).to_ascii_lowercase();
    if matches!(unsigned.as_str(), "inf" | "infinity" | "nan") {
        return s.parse::<f64>().map_err(|_| fail());
    }
    if !unsigned
        .bytes()
        .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'+' | b'-'))
    {
        return Err(fail());
    }
    let v: f64 = s.parse().map_err(|_| fail())?;
    if v.is_infinite() {
        return Err(fail());
    }
    Ok(v)
}

/// Go `hex.DecodeString`.
fn decode_hex(s: &str) -> Option<Vec<u8>> {
    let b = s.as_bytes();
    if !b.len().is_multiple_of(2) {
        return None;
    }
    b.chunks_exact(2)
        .map(|p| Some(((p[0] as char).to_digit(16)? << 4 | (p[1] as char).to_digit(16)?) as u8))
        .collect()
}

/// Go `base64.StdEncoding.DecodeString`: padded standard alphabet; `\r` and
/// `\n` are ignored; trailing bits need not be zero.
fn decode_base64(s: &str) -> Option<Vec<u8>> {
    let symbols: Vec<u8> = s.bytes().filter(|&c| c != b'\r' && c != b'\n').collect();
    if !symbols.len().is_multiple_of(4) {
        return None;
    }
    let value = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    };
    let mut out = Vec::with_capacity(symbols.len() / 4 * 3);
    let quads = symbols.len() / 4;
    for (i, quad) in symbols.chunks_exact(4).enumerate() {
        let pad = quad.iter().rev().take_while(|&&c| c == b'=').count();
        if pad > 2 || (pad > 0 && i + 1 != quads) {
            return None;
        }
        let mut n: u32 = 0;
        for &c in &quad[..4 - pad] {
            n = n << 6 | u32::from(value(c)?);
        }
        n <<= 6 * pad as u32;
        let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        out.extend_from_slice(&bytes[..3 - pad]);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(json: &str, opts: JsonOptions) -> Result<Kv, Error> {
        decode_json(json.as_bytes(), opts)
    }

    fn value(kv: &Kv, key: &str) -> Value {
        match &kv[key] {
            KvEntry::Value(v) => v.clone(),
            _ => panic!("not a value"),
        }
    }

    #[test]
    fn numbers_follow_go() {
        let kv = decode(
            r#"{"a": 1, "b": 18446744073709551615, "c": 1.5, "d": 1e3, "e": -1}"#,
            JsonOptions::default(),
        )
        .unwrap();
        assert_eq!(value(&kv, "a"), Value::Int(1));
        assert_eq!(value(&kv, "b"), Value::Uint(u64::MAX));
        assert_eq!(value(&kv, "c"), Value::Float(1.5));
        assert_eq!(value(&kv, "d"), Value::Float(1000.0));
        assert_eq!(value(&kv, "e"), Value::Int(-1));
        assert!(decode(r#"{"a": 18446744073709551616}"#, JsonOptions::default()).is_err());
        assert!(decode(r#"{"a": -9223372036854775809}"#, JsonOptions::default()).is_err());
        assert!(decode(r#"{"a": 1e400}"#, JsonOptions::default()).is_err());
        assert!(decode(r#"{"a": 01}"#, JsonOptions::default()).is_err());
    }

    #[test]
    fn reader_follows_go() {
        let kv = decode("{\"a\": 1, \"a\": 2} trailing", JsonOptions::default()).unwrap();
        assert_eq!(value(&kv, "a"), Value::Int(2));
        let kv = decode(
            r#"{"s": "\ud83d\ude00 \ud800 \u00e9"}"#,
            JsonOptions::default(),
        )
        .unwrap();
        assert_eq!(value(&kv, "s"), Value::String("😀 \u{FFFD} é".into()));
        let kv: Kv = decode_json(b"{\"s\": \"a\xffb\"}", JsonOptions::default()).unwrap();
        assert_eq!(value(&kv, "s"), Value::String("a\u{FFFD}b".into()));
        assert!(decode("[1]", JsonOptions::default()).is_err());
        assert!(decode("", JsonOptions::default()).is_err());
    }

    #[test]
    fn annotated_and_typed() {
        let opts = JsonOptions {
            annotated_keys: true,
            ..Default::default()
        };
        let kv = decode(
            r#"{"n.$int64": 1e3, "m.$uint64": "7", "b.$base64": "UE8=", "h.$hex": "50:4f"}"#,
            opts,
        )
        .unwrap();
        assert_eq!(value(&kv, "n"), Value::Int(1000));
        assert_eq!(value(&kv, "m"), Value::Uint(7));
        assert_eq!(value(&kv, "b"), Value::Bytes(b"PO".to_vec()));
        assert_eq!(value(&kv, "h"), Value::Bytes(b"PO".to_vec()));
        assert!(decode(r#"{"n.$int64": 1.5}"#, opts).is_err());
        assert!(decode(r#"{"src": 1, "src.$ip": "1.2.3.4"}"#, opts).is_err());

        let typed = JsonOptions {
            typed_document: true,
            ..Default::default()
        };
        let kv = decode(
            r#"{"u": {"$type": "uint64", "value": "18446744073709551615"}}"#,
            typed,
        )
        .unwrap();
        assert_eq!(value(&kv, "u"), Value::Uint(u64::MAX));
        assert!(decode(r#"{"i": {"$type": "int64", "value": 1.0}}"#, typed).is_err());
        assert!(decode(r#"{"n": {"$type": "null"}}"#, typed).is_err());
        assert!(decode(r#"{"x.$ip": {"$type": "ip", "value": "1.2.3.4"}}"#, typed).is_err());
    }

    #[test]
    fn base64_like_go() {
        assert_eq!(decode_base64("UE8="), Some(b"PO".to_vec()));
        assert_eq!(decode_base64("UE\n8="), Some(b"PO".to_vec()));
        assert_eq!(decode_base64("UE8"), None);
        assert_eq!(decode_base64("U=E8"), None);
        assert_eq!(decode_base64("UE9="), Some(b"PO".to_vec()));
    }
}
