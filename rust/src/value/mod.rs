//! Rule values: owned [`Value`], borrowed [`ValueRef`], and [`Val`] (either),
//! plus the typed network values [`Ip`], [`Cidr`], [`Mac`], and [`Url`].
//!
//! ```rust
//! use rulekit::value::{Cidr, Ip, Map, Url, Value};
//!
//! let input: Map<Value> = Map::from_iter([
//!     ("src".to_owned(), Value::Ip(Ip::parse("10.1.2.3").unwrap())),
//!     ("net".to_owned(), Value::Cidr(Cidr::parse("10.0.0.0/8").unwrap())),
//!     ("url".to_owned(), Value::Url(Box::new(Url::parse("https://example.com/a")?))),
//!     ("tags".to_owned(), Value::Array(vec![Value::String("prod".into())])),
//! ]);
//! # let _ = input;
//! # Ok::<(), String>(())
//! ```

mod fields;
mod ip;
mod mac;
mod query;
mod text;
mod url;

use std::collections::HashMap;

pub(crate) use fields::value_field;
pub use ip::{Cidr, Ip};
pub use mac::Mac;
pub use text::TextForm;
pub use url::Url;

/// A string-keyed map (fast, non-cryptographic hashing).
pub type Map<V> = HashMap<String, V, foldhash::fast::RandomState>;

/// An owned value: input data, a literal, or a function result.
#[derive(Clone, Debug, Default)]
pub enum Value {
    /// `null`.
    #[default]
    Null,
    /// A boolean.
    Bool(bool),
    /// A signed integer (`int64`).
    Int(i64),
    /// An unsigned integer (`uint64`); compares exactly with `Int`.
    Uint(u64),
    /// A float (`float64`).
    Float(f64),
    /// A UTF-8 string.
    String(String),
    /// Byte strings, including hex literals.
    Bytes(Vec<u8>),
    /// An IP address.
    Ip(Ip),
    /// A CIDR block.
    Cidr(Cidr),
    /// A MAC address.
    Mac(Mac),
    /// A URL.
    Url(Box<Url>),
    /// A compiled regex.
    Regex(Box<regex::Regex>),
    /// A list.
    Array(Vec<Value>),
    /// A string-keyed object.
    Object(Map<Value>),
}

impl Value {
    /// Borrow the value.
    pub fn as_ref(&self) -> ValueRef<'_> {
        match self {
            Value::Null => ValueRef::Null,
            Value::Bool(b) => ValueRef::Bool(*b),
            Value::Int(n) => ValueRef::Int(*n),
            Value::Uint(n) => ValueRef::Uint(*n),
            Value::Float(n) => ValueRef::Float(*n),
            Value::String(s) => ValueRef::Str(s),
            Value::Bytes(b) => ValueRef::Bytes(b),
            Value::Ip(ip) => ValueRef::Ip(*ip),
            Value::Cidr(c) => ValueRef::Cidr(*c),
            Value::Mac(m) => ValueRef::Mac(*m),
            Value::Url(u) => ValueRef::Url(u),
            Value::Regex(r) => ValueRef::Regex(r),
            Value::Array(a) => ValueRef::Array(ArrayRef::Values(a)),
            Value::Object(m) => ValueRef::Object(ObjectRef::Map(m)),
        }
    }
}

impl PartialEq for Value {
    /// Structural equality; regexes compare by pattern.
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Null, Value::Null) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Uint(a), Value::Uint(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Bytes(a), Value::Bytes(b)) => a == b,
            (Value::Ip(a), Value::Ip(b)) => a == b,
            (Value::Cidr(a), Value::Cidr(b)) => a == b,
            (Value::Mac(a), Value::Mac(b)) => a == b,
            (Value::Url(a), Value::Url(b)) => a == b,
            (Value::Regex(a), Value::Regex(b)) => a.as_str() == b.as_str(),
            (Value::Array(a), Value::Array(b)) => a == b,
            (Value::Object(a), Value::Object(b)) => a == b,
            _ => false,
        }
    }
}

/// A borrowed view of a value. Small values are held by copy.
#[derive(Clone, Copy, Debug)]
pub enum ValueRef<'a> {
    /// `null`.
    Null,
    /// A boolean.
    Bool(bool),
    /// A signed integer.
    Int(i64),
    /// An unsigned integer.
    Uint(u64),
    /// A float.
    Float(f64),
    /// A string.
    Str(&'a str),
    /// A URL's raw query string: compares as text, and has parameter fields.
    Query(&'a str),
    /// A byte string.
    Bytes(&'a [u8]),
    /// An IP address.
    Ip(Ip),
    /// A CIDR block.
    Cidr(Cidr),
    /// A MAC address.
    Mac(Mac),
    /// A URL.
    Url(&'a Url),
    /// A compiled regex.
    Regex(&'a regex::Regex),
    /// A list.
    Array(ArrayRef<'a>),
    /// An object.
    Object(ObjectRef<'a>),
}

/// A borrowed list of values.
#[derive(Clone, Copy, Debug)]
pub enum ArrayRef<'a> {
    /// Items of a [`Value::Array`].
    Values(&'a [Value]),
    /// Items computed during evaluation.
    Vals(&'a [Val<'a>]),
}

impl<'a> ArrayRef<'a> {
    /// The number of items.
    pub fn len(&self) -> usize {
        match self {
            ArrayRef::Values(v) => v.len(),
            ArrayRef::Vals(v) => v.len(),
        }
    }

    /// Whether the list has no items.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The item at index `i`.
    pub fn get(&self, i: usize) -> Option<ValueRef<'a>> {
        match self {
            ArrayRef::Values(v) => v.get(i).map(Value::as_ref),
            ArrayRef::Vals(v) => v.get(i).map(Val::as_ref),
        }
    }

    /// The items in order.
    pub fn iter(&self) -> impl Iterator<Item = ValueRef<'a>> + 'a {
        let this = *self;
        (0..this.len()).map(move |i| this.get(i).expect("index in range"))
    }
}

/// A borrowed object.
#[derive(Clone, Copy, Debug)]
pub enum ObjectRef<'a> {
    /// A plain object.
    Map(&'a Map<Value>),
    /// A map holding inputs or lazy values, or a nested input, reached as a
    /// value. It is truthy and compares with nothing.
    Opaque,
}

/// A borrowed or owned value: what inputs, functions, and evaluation return.
///
/// Borrow when the data outlives the evaluation (input data, the context);
/// own computed values. `From` converts a [`Value`] or [`ValueRef`].
#[derive(Clone, Debug)]
pub enum Val<'a> {
    /// A borrowed value.
    Ref(ValueRef<'a>),
    /// An owned value.
    Owned(Value),
}

impl<'a> Val<'a> {
    /// Borrow the value.
    pub fn as_ref(&self) -> ValueRef<'_> {
        match self {
            Val::Ref(v) => *v,
            Val::Owned(v) => v.as_ref(),
        }
    }

    /// Convert to an owned value (an opaque object becomes `Null`).
    pub fn into_owned(self) -> Value {
        match self {
            Val::Ref(v) => v.to_owned(),
            Val::Owned(v) => v,
        }
    }
}

impl From<Value> for Val<'_> {
    fn from(v: Value) -> Self {
        Val::Owned(v)
    }
}

impl<'a> From<ValueRef<'a>> for Val<'a> {
    fn from(v: ValueRef<'a>) -> Self {
        Val::Ref(v)
    }
}

impl<'a> ValueRef<'a> {
    /// Copy into an owned value (an opaque object becomes `Null`).
    #[allow(clippy::wrong_self_convention)]
    pub fn to_owned(self) -> Value {
        match self {
            ValueRef::Null | ValueRef::Object(ObjectRef::Opaque) => Value::Null,
            ValueRef::Bool(b) => Value::Bool(b),
            ValueRef::Int(n) => Value::Int(n),
            ValueRef::Uint(n) => Value::Uint(n),
            ValueRef::Float(n) => Value::Float(n),
            ValueRef::Str(s) | ValueRef::Query(s) => Value::String(s.to_owned()),
            ValueRef::Bytes(b) => Value::Bytes(b.to_vec()),
            ValueRef::Ip(ip) => Value::Ip(ip),
            ValueRef::Cidr(c) => Value::Cidr(c),
            ValueRef::Mac(m) => Value::Mac(m),
            ValueRef::Url(u) => Value::Url(Box::new(u.clone())),
            ValueRef::Regex(r) => Value::Regex(Box::new(r.clone())),
            ValueRef::Array(a) => Value::Array(a.iter().map(ValueRef::to_owned).collect()),
            ValueRef::Object(ObjectRef::Map(m)) => Value::Object(m.clone()),
        }
    }

    /// Whether the value is zero: null, false, 0, or an empty
    /// string/bytes/array. Typed network values, regexes, and objects are
    /// non-zero. A rule passes when its value is non-zero.
    pub fn is_zero(self) -> bool {
        match self {
            ValueRef::Null => true,
            ValueRef::Bool(b) => !b,
            ValueRef::Int(n) => n == 0,
            ValueRef::Uint(n) => n == 0,
            ValueRef::Float(n) => n == 0.0,
            ValueRef::Str(s) | ValueRef::Query(s) => s.is_empty(),
            ValueRef::Bytes(b) => b.is_empty(),
            ValueRef::Array(a) => a.is_empty(),
            ValueRef::Ip(_)
            | ValueRef::Cidr(_)
            | ValueRef::Mac(_)
            | ValueRef::Url(_)
            | ValueRef::Regex(_)
            | ValueRef::Object(_) => false,
        }
    }

    /// The value's type name, using the typed-JSON vocabulary (`int64`,
    /// `string`, `ip`, ...).
    pub fn type_name(self) -> &'static str {
        match self {
            ValueRef::Null => "null",
            ValueRef::Bool(_) => "bool",
            ValueRef::Int(_) => "int64",
            ValueRef::Uint(_) => "uint64",
            ValueRef::Float(_) => "float64",
            ValueRef::Str(_) | ValueRef::Query(_) => "string",
            ValueRef::Bytes(_) => "bytes",
            ValueRef::Ip(_) => "ip",
            ValueRef::Cidr(_) => "cidr",
            ValueRef::Mac(_) => "mac",
            ValueRef::Url(_) => "url",
            ValueRef::Regex(_) => "regex",
            ValueRef::Array(_) => "array",
            ValueRef::Object(_) => "object",
        }
    }

    /// The text a value compares with strings by: strings themselves and the
    /// text forms of IPs, CIDRs, MACs, URLs, and URL queries.
    pub fn text(self) -> Option<TextForm<'a>> {
        match self {
            ValueRef::Str(s) | ValueRef::Query(s) => Some(TextForm::Borrowed(s)),
            ValueRef::Url(u) => Some(TextForm::Borrowed(u.as_str())),
            ValueRef::Ip(ip) => Some(TextForm::display(ip)),
            ValueRef::Cidr(c) => Some(TextForm::display(c)),
            ValueRef::Mac(m) => Some(TextForm::display(m)),
            _ => None,
        }
    }
}
