//! JSON input decoding (`json.go`).
//!
//! `serde_json` reads the document. Every member and element is first captured
//! as a borrowed [`RawValue`], so numbers keep their source text (Go
//! `UseNumber`): an integer outside int64/uint64 is an error rather than a
//! float, and a number in a field that is never converted is never parsed.
//! Objects and arrays are then read from their raw text, one level at a time;
//! the shared depth limit (100 levels) bounds the rescanning that implies.
//!
//! Reader rules: exactly one JSON value with only whitespace around it; the
//! input must be valid UTF-8 and string escapes must not encode lone
//! surrogates; a repeated key keeps its last value.

use std::borrow::Cow;
use std::cell::Cell;
use std::fmt;

use base64::Engine as _;
use serde::de::{self, Deserialize, Deserializer as _, MapAccess, SeqAccess, Visitor};
use serde_json::value::RawValue;

use crate::error::Error;
use crate::input::{Kv, KvEntry};
use crate::literal::is_float;
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
    check_depth(data)?;
    let text = std::str::from_utf8(data).map_err(|e| err(format!("invalid json: {e}")))?;
    // Checks the syntax of the whole document, including trailing data.
    let raw: &RawValue = serde_json::from_str(text).map_err(json_err)?;
    let failed = Cell::new(None);
    let raw = Reader {
        input: text,
        failed: &failed,
    }
    .node(raw)?;
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

fn json_err(e: serde_json::Error) -> Error {
    err(format!("invalid json: {e}"))
}

fn ctx(prefix: String, e: Error) -> Error {
    match e {
        Error::Json(msg) => Error::Json(format!("{prefix}: {msg}")),
        other => other,
    }
}

/// Deepest nesting of objects and arrays, counting the root.
const MAX_DEPTH: usize = 100;

/// Reject documents nested deeper than [`MAX_DEPTH`] (Go `checkJSONDepth`).
/// Brackets inside strings are skipped; syntax errors are left to the reader.
fn check_depth(data: &[u8]) -> Result<(), Error> {
    let (mut depth, mut in_string, mut escaped) = (0usize, false, false);
    for &c in data {
        if in_string {
            if escaped {
                escaped = false;
            } else if c == b'\\' {
                escaped = true;
            } else if c == b'"' {
                in_string = false;
            }
            continue;
        }
        match c {
            b'"' => in_string = true,
            b'{' | b'[' => {
                depth += 1;
                if depth > MAX_DEPTH {
                    return Err(err(format!("json nesting exceeds {MAX_DEPTH} levels")));
                }
            }
            b'}' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    Ok(())
}

/// A parsed JSON value; numbers keep their source text.
enum Json<'a> {
    Null,
    Bool(bool),
    Number(&'a str),
    String(String),
    Array(Vec<Json<'a>>),
    Object(Map<Json<'a>>),
}

impl Json<'_> {
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

/// Reads raw values of `input` into [`Json`] trees.
#[derive(Clone, Copy)]
struct Reader<'a, 'f> {
    input: &'a str,
    /// The error from a nested [`Reader::node`], which serde can only carry
    /// out as a message.
    failed: &'f Cell<Option<Error>>,
}

impl<'a> Reader<'a, '_> {
    /// Read one raw value. Its text is a single valid value without
    /// surrounding whitespace, so the first byte names its kind. The syntax
    /// was checked when the root was read; decoding a string or key can still
    /// fail (a lone surrogate escape).
    fn node(self, raw: &'a RawValue) -> Result<Json<'a>, Error> {
        let text = raw.get();
        let mut de = serde_json::Deserializer::from_str(text);
        let parsed = match text.as_bytes().first() {
            Some(b'{') => de.deserialize_map(Members(self)).map(Json::Object),
            Some(b'[') => de.deserialize_seq(Elements(self)).map(Json::Array),
            Some(b'"') => String::deserialize(&mut de).map(Json::String),
            Some(b't') => Ok(Json::Bool(true)),
            Some(b'f') => Ok(Json::Bool(false)),
            Some(b'n') => Ok(Json::Null),
            _ => Ok(Json::Number(text)),
        };
        parsed.map_err(|e| self.failed.take().unwrap_or_else(|| self.locate(text, &e)))
    }

    /// A nested value, for a visitor: its error goes out of band.
    fn child<E: de::Error>(self, raw: &'a RawValue) -> Result<Json<'a>, E> {
        self.node(raw).map_err(|e| {
            self.failed.set(Some(e));
            E::custom("invalid nested value")
        })
    }

    /// Report `e`, positioned within `text`, at its line and column in the
    /// whole input.
    fn locate(self, text: &str, e: &serde_json::Error) -> Error {
        let start = text.as_ptr().addr() - self.input.as_ptr().addr();
        let before = &self.input[..start];
        let line = before.matches('\n').count() + e.line();
        let column = match e.line() {
            1 => start - before.rfind('\n').map_or(0, |i| i + 1) + e.column(),
            _ => e.column(),
        };
        let full = e.to_string();
        let suffix = format!(" at line {} column {}", e.line(), e.column());
        let msg = full.strip_suffix(&suffix).unwrap_or(&full);
        err(format!(
            "invalid json: {msg} at line {line} column {column}"
        ))
    }
}

/// Visits one object level; member values are read by [`Reader::node`].
struct Members<'a, 'f>(Reader<'a, 'f>);

impl<'a> Visitor<'a> for Members<'a, '_> {
    type Value = Map<Json<'a>>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a JSON object")
    }

    fn visit_map<A: MapAccess<'a>>(self, mut access: A) -> Result<Self::Value, A::Error> {
        let mut map = Map::default();
        while let Some(key) = access.next_key::<String>()? {
            let raw: &'a RawValue = access.next_value()?;
            map.insert(key, self.0.child(raw)?);
        }
        Ok(map)
    }
}

/// Visits one array level; elements are read by [`Reader::node`].
struct Elements<'a, 'f>(Reader<'a, 'f>);

impl<'a> Visitor<'a> for Elements<'a, '_> {
    type Value = Vec<Json<'a>>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a JSON array")
    }

    fn visit_seq<A: SeqAccess<'a>>(self, mut access: A) -> Result<Self::Value, A::Error> {
        let mut items = Vec::with_capacity(access.size_hint().unwrap_or(0));
        while let Some(raw) = access.next_element::<&'a RawValue>()? {
            items.push(self.0.child(raw)?);
        }
        Ok(items)
    }
}

/// Go `normalizePlainJSONValue`.
fn plain(value: Json, annotated: bool) -> Result<Value, Error> {
    Ok(match value {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Bool(b),
        Json::String(s) => Value::String(s),
        Json::Number(n) => number(n)?,
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

/// Go `normalizeJSONNumber`: a number with a fraction or exponent is a float,
/// any other is an int64, else a uint64, else an error.
fn number(n: &str) -> Result<Value, Error> {
    if n.contains(['.', 'e', 'E']) {
        return parse_float(n).map(Value::Float);
    }
    n.parse()
        .map(Value::Int)
        .or_else(|_| n.parse().map(Value::Uint))
        .map_err(|_| err(format!("invalid json number {n:?}")))
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
    Json(&'a Json<'a>),
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
    fn numeric(self) -> Result<Cow<'a, str>, Error> {
        match self {
            Raw::Json(Json::Number(n)) => Ok(Cow::Borrowed(n)),
            Raw::Json(Json::String(s)) | Raw::Value(Value::String(s)) => Ok(Cow::Borrowed(s)),
            Raw::Value(Value::Int(n)) => Ok(Cow::Owned(n.to_string())),
            Raw::Value(Value::Uint(n)) => Ok(Cow::Owned(n.to_string())),
            // Rust's float Display, like Go strconv.FormatFloat(v, 'f', -1,
            // 64), prints the shortest round-trip digits without an exponent.
            Raw::Value(Value::Float(n)) => Ok(Cow::Owned(n.to_string())),
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
            Value::Int(
                s.parse()
                    .map_err(|e| err(format!("invalid int64 {s:?}: {e}")))?,
            )
        }
        "uint64" => {
            let s = value.numeric()?;
            Value::Uint(
                s.parse()
                    .map_err(|e| err(format!("invalid uint64 {s:?}: {e}")))?,
            )
        }
        "float64" => {
            let s = value.numeric()?;
            if !is_decimal(&s) {
                return Err(err(format!("invalid float64 {s:?}")));
            }
            Value::Float(parse_float(&s)?)
        }
        _ => return Err(err(format!("unknown type {typ:?}"))),
    })
}

fn bytes(value: Raw<'_>, encoding: &str) -> Result<Value, Error> {
    let s = value.string()?;
    match encoding {
        "hex" => decode_hex(&s.replace(':', ""))
            .map(Value::Bytes)
            .ok_or_else(|| err("invalid hex bytes")),
        "base64" => base64::engine::general_purpose::STANDARD
            .decode(s)
            .map(Value::Bytes)
            .map_err(|e| err(format!("invalid base64 bytes: {e}"))),
        _ => Err(err("bytes encoding must be hex or base64")),
    }
}

/// A decimal number: optional sign, digits, optional fraction and exponent
/// (Go `isFloat || isDecimalInteger`). No hex floats, `inf`, `nan`, or `_`.
fn is_decimal(s: &str) -> bool {
    let digits = s.strip_prefix(['+', '-']).unwrap_or(s);
    is_float(s) || (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
}

/// Parse decimal float text; overflow to infinity is an error (as Go
/// `strconv.ParseFloat`), underflow to zero is not.
fn parse_float(s: &str) -> Result<f64, Error> {
    match s.parse::<f64>() {
        Ok(v) if v.is_finite() => Ok(v),
        _ => Err(err(format!("invalid float64 {s:?}"))),
    }
}

/// Hex pairs, either case; an odd length or a non-hex digit is an error.
fn decode_hex(s: &str) -> Option<Vec<u8>> {
    let b = s.as_bytes();
    if !b.len().is_multiple_of(2) {
        return None;
    }
    b.chunks_exact(2)
        .map(|p| Some(((p[0] as char).to_digit(16)? << 4 | (p[1] as char).to_digit(16)?) as u8))
        .collect()
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
    fn reader_rules() {
        let plain = JsonOptions::default();
        let kv = decode(r#"{"a": 1, "a": 2}"#, plain).unwrap();
        assert_eq!(value(&kv, "a"), Value::Int(2));
        let kv = decode(" {\"s\": \"\\ud83d\\ude00 \\u00e9\"}\r\n\t", plain).unwrap();
        assert_eq!(value(&kv, "s"), Value::String("😀 é".into()));
        assert!(decode(r#"{"a": 1} trailing"#, plain).is_err());
        assert!(decode(r#"{"a": 1} {}"#, plain).is_err());
        assert!(decode(r#"{"s": "\ud800"}"#, plain).is_err());
        assert!(decode(r#"{"\udc00": 1}"#, plain).is_err());
        assert!(decode_json::<()>(b"{\"s\": \"a\xffb\"}", plain).is_err());
        assert!(decode("\u{feff}{}", plain).is_err());
        assert!(decode("[1]", plain).is_err());
        assert!(decode("", plain).is_err());
        // Unconverted numbers in typed documents are never parsed.
        let typed = JsonOptions {
            typed_document: true,
            ..Default::default()
        };
        let kv = decode(
            r#"{"s": {"$type": "string", "value": "x", "n": 1e400, "m": 123456789012345678901234567890}}"#,
            typed,
        )
        .unwrap();
        assert_eq!(value(&kv, "s"), Value::String("x".into()));
    }

    #[test]
    fn depth_limit() {
        let nested = |n: usize| format!("{}{}", "{\"a\":".repeat(n) + "1", "}".repeat(n));
        assert!(decode(&nested(100), JsonOptions::default()).is_ok());
        assert!(decode(&nested(101), JsonOptions::default()).is_err());
        let arrays = format!("{{\"a\":{}1{}}}", "[".repeat(99), "]".repeat(99));
        assert!(decode(&arrays, JsonOptions::default()).is_ok());
        let arrays = format!("{{\"a\":{}1{}}}", "[".repeat(100), "]".repeat(100));
        assert!(decode(&arrays, JsonOptions::default()).is_err());
        // Brackets inside strings do not count.
        let s = format!("{{\"a\":\"{}\\\"{}\"}}", "[".repeat(200), "{".repeat(200));
        assert!(decode(&s, JsonOptions::default()).is_ok());
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

    fn typed(typ: &str, value: &str) -> Result<Value, Error> {
        let doc = format!(r#"{{"v": {{"$type": "{typ}", "value": {value}}}}}"#);
        let opts = JsonOptions {
            typed_document: true,
            ..Default::default()
        };
        decode(&doc, opts).map(|kv| self::value(&kv, "v"))
    }

    #[test]
    fn typed_integers() {
        assert_eq!(typed("int64", r#""+7""#).unwrap(), Value::Int(7));
        assert_eq!(typed("int64", r#""-007""#).unwrap(), Value::Int(-7));
        assert_eq!(typed("int64", r#""-0""#).unwrap(), Value::Int(0));
        assert_eq!(typed("int64", "-0").unwrap(), Value::Int(0));
        assert_eq!(
            typed("int64", r#""-9223372036854775808""#).unwrap(),
            Value::Int(i64::MIN)
        );
        // Proposed shared rule: uint64 accepts a leading `+` like int64.
        assert_eq!(typed("uint64", r#""+7""#).unwrap(), Value::Uint(7));
        for bad in [
            r#""""#,
            r#""+""#,
            r#""-""#,
            r#"" 5""#,
            r#""5 ""#,
            r#""1_000""#,
            r#""0x10""#,
            r#""1e3""#,
            r#""1.0""#,
            "1e3",
            "1.0",
            r#""++1""#,
            r#""١""#,
            "true",
            "null",
            r#""9223372036854775808""#,
        ] {
            assert!(typed("int64", bad).is_err(), "int64 {bad}");
        }
        for bad in [
            r#""-0""#,
            r#""-1""#,
            "-0",
            r#""18446744073709551616""#,
            r#""+-1""#,
        ] {
            assert!(typed("uint64", bad).is_err(), "uint64 {bad}");
        }
    }

    #[test]
    fn typed_floats() {
        for (text, want) in [
            (r#""1e3""#, 1000.0),
            (r#""5.""#, 5.0),
            (r#""+1.5""#, 1.5),
            (r#""-0""#, -0.0),
            (r#""007""#, 7.0),
            (r#""1e-400""#, 0.0),
            (r#""1.7976931348623158e308""#, f64::MAX),
            ("1e3", 1000.0),
        ] {
            assert_eq!(
                typed("float64", text).unwrap(),
                Value::Float(want),
                "{text}"
            );
        }
        for bad in [
            r#"".5""#,
            r#""1e""#,
            r#""0x1p3""#,
            r#""inf""#,
            r#""NaN""#,
            r#""1_0.0""#,
            r#""1e400""#,
            r#""1.7976931348623159e308""#,
            r#"" 1""#,
            r#""""#,
        ] {
            assert!(typed("float64", bad).is_err(), "float64 {bad}");
        }
        // Annotated values are normalized first, then printed without an
        // exponent before conversion.
        let opts = JsonOptions {
            annotated_keys: true,
            ..Default::default()
        };
        let kv = decode(
            r#"{"a.$int64": 1e18, "b.$uint64": -0.0, "c.$int64": -0.0}"#,
            opts,
        );
        assert!(kv.is_err());
        let kv = decode(
            r#"{"a.$int64": 1e18, "c.$int64": -0.0, "f.$float64": 1e-7}"#,
            opts,
        )
        .unwrap();
        assert_eq!(value(&kv, "a"), Value::Int(1_000_000_000_000_000_000));
        assert_eq!(value(&kv, "c"), Value::Int(0));
        assert_eq!(value(&kv, "f"), Value::Float(1e-7));
        assert!(decode(r#"{"a.$int64": 1e19}"#, opts).is_err());
    }

    #[test]
    fn bytes_are_canonical() {
        let b64 = |s: &str| typed("base64", &format!("{s:?}"));
        assert_eq!(b64("UE8=").unwrap(), Value::Bytes(b"PO".to_vec()));
        assert_eq!(b64("").unwrap(), Value::Bytes(Vec::new()));
        // Proposed shared rule: padding required, trailing bits zero, no
        // line breaks.
        for bad in [
            "UE8", "UE9=", "UE==", "U===", "UE8=\n", "U\r\nE8=", "UE8-", "UE8=UE8=",
        ] {
            assert!(b64(bad).is_err(), "base64 {bad:?}");
        }
        let hex = |s: &str| typed("hex", &format!("{s:?}"));
        assert_eq!(hex("0A:0b").unwrap(), Value::Bytes(vec![10, 11]));
        assert_eq!(hex("0:a:0:b").unwrap(), Value::Bytes(vec![10, 11]));
        for bad in ["a", "0:a0", "0g", "+f", " 0a", "0x0a"] {
            assert!(hex(bad).is_err(), "hex {bad:?}");
        }
    }

    #[test]
    fn nested_errors_are_positioned_in_the_input() {
        let e = decode(
            "{\"a\": 1,\n  \"b\": [\"x\", \"\\ud800\"]}",
            JsonOptions::default(),
        );
        let Err(Error::Json(msg)) = e else {
            panic!("expected a JSON error");
        };
        assert_eq!(
            msg,
            "invalid json: unexpected end of hex escape at line 2 column 21"
        );
    }
}
