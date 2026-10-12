//! [`SerdeInput`]: read any [`serde::Serialize`] value as an input.
//!
//! A path is resolved by running `T::serialize` into a serializer that
//! follows it. Struct fields, map keys, enum variants, and sequence positions
//! that are not on the path are skipped without serializing them, and the walk
//! ignores everything after the target.

use std::fmt::{self, Display};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use ipnet::{IpNet, Ipv4Net, Ipv6Net};
use serde::ser::{
    self, Impossible, Serialize, SerializeMap, SerializeSeq, SerializeStruct,
    SerializeStructVariant, SerializeTuple, SerializeTupleStruct, SerializeTupleVariant,
    Serializer,
};

use crate::ast::Segment;
use crate::error::BoxError;
use crate::input::Input;
use crate::input_value::InputValue;
use crate::value::{Cidr, Ip, Mac, Map, ObjectRef, Url, Val, Value, ValueRef};

/// Reads any [`Serialize`] value as an input, with the paths of its
/// `serde_json` form.
///
/// Use it when the type cannot (or should not) depend on rulekit: plain
/// `#[derive(Serialize)]` structs and enums, types from other crates,
/// anything with a hand-written `Serialize`. `#[derive(Input)]` stays the
/// choice when you own the type: it reads fields in place, borrows strings,
/// and never allocates, but needs the dependency. Here every field read is
/// one walk through `T::serialize`, and the value read is owned (serde lends
/// a string for one callback only).
///
/// ```rust
/// use rulekit::{Opts, SerdeInput};
/// use serde::Serialize;
///
/// #[derive(Serialize)]
/// #[serde(tag = "type", content = "payload")]
/// enum Event {
///     #[serde(rename = "llm.request")]
///     Request {
///         model: String,
///         ip: std::net::IpAddr,
///         #[serde(skip_serializing_if = "Option::is_none")]
///         request_id: Option<String>,
///     },
/// }
///
/// let event = Event::Request {
///     model: "gpt-x".into(),
///     ip: "10.1.2.3".parse().unwrap(),
///     request_id: None,
/// };
/// // serde_json: {"type":"llm.request","payload":{"model":"gpt-x","ip":"10.1.2.3"}}
/// let input = SerdeInput::new(&event);
/// let opts = Opts::default();
/// let rule = rulekit::parse(
///     r#"type == "llm.request" and payload.model == "gpt-x" and payload.ip in 10.0.0.0/8"#,
/// )?;
/// assert!(rule.eval(&input, &opts).pass());
///
/// let rule = rulekit::parse(r#"payload.request_id == "r1""#)?;
/// let result = rule.eval(&input, &opts);
/// assert_eq!(result.missing_fields().collect::<Vec<_>>(), ["payload.request_id"]);
/// # Ok::<(), rulekit::ParseError>(())
/// ```
///
/// # Paths follow serde
///
/// The walk reads what `T::serialize` writes, so names and layout are
/// exactly serde's: `rename`, `rename_all`, `tag`, `tag` + `content`,
/// `untagged`, `flatten`, `skip_serializing_if` (an omitted field is
/// missing), `transparent`, newtypes, and custom `Serialize` impls. Maps
/// match string keys, and integer, `bool`, and `char` keys by their decimal
/// or text form, as `serde_json` writes them. When a key occurs twice (a
/// named field and a flattened entry), the first one wins.
///
/// - A struct, map, or enum variant that holds data, read whole, is an opaque
///   object, as for `#[derive(Input)]`; index or key into it instead.
/// - A sequence, tuple, or tuple struct read whole is an array of its items,
///   so `tags contains "db"` works. An item that is an object is an empty
///   object there; read `items[0].name` to see inside.
/// - `None` is `null` (a path below it is missing), a unit variant is its
///   name, `Vec<u8>` is a list of numbers, and `serialize_bytes` (as
///   `serde_bytes` writes) is a byte string. Integers beyond `u64` are an
///   input error.
///
/// # Typed leaves
///
/// serde writes an `IpAddr` as a string, but a rule comparing it with
/// `10.0.0.0/8` needs an IP. When the value about to be read or descended
/// into is exactly one of these types, it is read as rulekit's typed value,
/// the same one its [`InputValue`] impl gives, with its built-in fields
/// (`ip.version`, `cidr.prefix`, `url.host`, ...):
///
/// - `std::net::{IpAddr, Ipv4Addr, Ipv6Addr}`
/// - `ipnet::{IpNet, Ipv4Net, Ipv6Net}`
/// - [`value::Ip`](crate::value::Ip), [`value::Cidr`](crate::value::Cidr),
///   [`value::Mac`](crate::value::Mac), [`value::Url`](crate::value::Url)
/// - `url::Url` (feature `url`)
///
/// A field of type `Option<IpAddr>` is typed too, as is an element of a
/// `Vec<IpAddr>`, a value of a map, or a `&IpAddr`: serde hands each to the
/// walk as the type itself, behind references. What a `Serialize` impl
/// forwards straight to the inner `serialize` (`Box`, `Rc`, `Arc`, `Cow`,
/// `#[serde(transparent)]`, untagged newtype variants)
/// never reaches the walk as that type: it writes the text, so it reads as a
/// string.
/// `ipnet` and `url` implement `Serialize` behind their own `serde` features.
///
/// `SerdeInput` is also an [`InputValue`], so it nests: a derived struct or
/// `kv!` entry can hold one.
pub struct SerdeInput<'a, T: ?Sized>(&'a T);

impl<'a, T: ?Sized> SerdeInput<'a, T> {
    /// Wraps `value`.
    pub fn new(value: &'a T) -> Self {
        SerdeInput(value)
    }
}

impl<T: ?Sized> Clone for SerdeInput<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: ?Sized> Copy for SerdeInput<'_, T> {}

impl<T: ?Sized> fmt::Debug for SerdeInput<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SerdeInput").finish_non_exhaustive()
    }
}

impl<T: ?Sized + Serialize> InputValue for SerdeInput<'_, T> {
    fn get<'a>(&'a self, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        Ok(visit(self.0, path)?.into_val())
    }
}

impl<T: ?Sized + Serialize> Input for SerdeInput<'_, T> {
    fn get<'a>(&'a self, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        InputValue::get(self, path)
    }
}

/// What a walk found at the end of the path.
enum Found {
    /// Nothing is there: a missing field.
    Missing,
    /// A value.
    Value(Value),
    /// A struct, map, or variant read whole.
    Opaque,
}

impl Found {
    fn into_val(self) -> Option<Val<'static>> {
        match self {
            Found::Missing => None,
            Found::Value(value) => Some(Val::Owned(value)),
            Found::Opaque => Some(Val::Ref(ValueRef::Object(ObjectRef::Opaque))),
        }
    }

    /// An array item. An opaque object stands in as an empty object, since
    /// [`Value`] has no opaque form.
    fn into_item(self) -> Value {
        match self {
            Found::Value(value) => value,
            Found::Opaque => Value::Object(Map::default()),
            Found::Missing => Value::Null,
        }
    }

    fn from_val(val: Val<'_>) -> Found {
        match val {
            Val::Owned(value) => Found::Value(value),
            Val::Ref(ValueRef::Object(_)) => Found::Opaque,
            Val::Ref(value) => Found::Value(value.to_owned()),
        }
    }
}

/// The value at `path` inside `value`: a typed leaf when `T` is one,
/// otherwise a walk through `T::serialize`.
fn visit<T: ?Sized + Serialize>(value: &T, path: &[Segment]) -> Result<Found, Error> {
    match typed(value, path) {
        Some(found) => found,
        None => value.serialize(Walk { path }),
    }
}

/// Types read as rulekit values instead of through `serialize`.
///
/// Only types without lifetime parameters may implement this: [`downcast`]
/// relies on it.
trait Leaf: InputValue + 'static {}

macro_rules! leaves {
    ($($(#[$cfg:meta])* $ty:ty),* $(,)?) => {
        $($(#[$cfg])* impl Leaf for $ty {})*

        /// `Some` when `T` is one of the leaf types.
        fn typed<T: ?Sized>(value: &T, path: &[Segment]) -> Option<Result<Found, Error>> {
            $(
                $(#[$cfg])*
                if let Some(leaf) = downcast::<T, $ty>(value) {
                    return Some(read_leaf(leaf, path));
                }
            )*
            None
        }
    };
}

leaves! {
    IpAddr,
    Ipv4Addr,
    Ipv6Addr,
    IpNet,
    Ipv4Net,
    Ipv6Net,
    Ip,
    Cidr,
    Mac,
    Url,
    #[cfg(feature = "url")]
    ::url::Url,
}

/// `value` as a `&U` when `T` is `U`, `&U`, or `&&U`.
///
/// serde hands elements and map values to the serializer by reference:
/// `Vec<IpAddr>` calls it with `&IpAddr` for each element.
fn downcast<T: ?Sized, U: Leaf>(value: &T) -> Option<&U> {
    let ptr = value as *const T;
    let id = typeid::of::<T>();
    // SAFETY, for each branch: `typeid::of::<T>()` is `T`'s `TypeId` with
    // every lifetime replaced by `'static`. `U: Leaf` has no lifetime
    // parameters (the contract of `Leaf`), so an id equal to that of `U`,
    // `&U`, or `&&U` means `T` is `U` or a reference to it, whatever the
    // lifetimes of the references. `ptr` is valid and aligned for `T` and
    // lives as long as `value`, so each pointer cast and dereference reads
    // the `U` that `value` holds or points to. The returned borrow lasts no
    // longer than `value`'s own, which every reference in `T` outlives.
    if id == typeid::of::<U>() {
        Some(unsafe { &*ptr.cast::<U>() })
    } else if id == typeid::of::<&U>() {
        Some(unsafe { &**ptr.cast::<&U>() })
    } else if id == typeid::of::<&&U>() {
        Some(unsafe { &***ptr.cast::<&&U>() })
    } else {
        None
    }
}

/// The leaf's own `InputValue` read, owned.
fn read_leaf<L: Leaf>(leaf: &L, path: &[Segment]) -> Result<Found, Error> {
    match InputValue::get(leaf, path) {
        Ok(Some(val)) => Ok(Found::from_val(val)),
        Ok(None) => Ok(Found::Missing),
        Err(err) => Err(Error(err.to_string())),
    }
}

#[derive(Debug)]
struct Error(String);

impl Error {
    fn msg(msg: &str) -> Error {
        Error(msg.to_owned())
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl ser::Error for Error {
    fn custom<T: Display>(msg: T) -> Error {
        Error(msg.to_string())
    }
}

/// The unsigned integer a rule sees: `int64` when it fits.
fn uint(n: u64) -> Value {
    i64::try_from(n).map_or(Value::Uint(n), Value::Int)
}

/// Follows `path` through the value being serialized.
///
/// With an empty path the serializer is reading the value itself.
#[derive(Clone, Copy)]
struct Walk<'p> {
    path: &'p [Segment],
}

impl Walk<'_> {
    /// A scalar. Nothing is below one, so `make` only runs for an empty path.
    fn scalar(self, make: impl FnOnce() -> Result<Value, Error>) -> Result<Found, Error> {
        if self.path.is_empty() {
            make().map(Found::Value)
        } else {
            Ok(Found::Missing)
        }
    }
}

impl<'p> Serializer for Walk<'p> {
    type Ok = Found;
    type Error = Error;
    type SerializeSeq = Items<'p>;
    type SerializeTuple = Items<'p>;
    type SerializeTupleStruct = Items<'p>;
    type SerializeTupleVariant = Items<'p>;
    type SerializeMap = Fields<'p>;
    type SerializeStruct = Fields<'p>;
    type SerializeStructVariant = Fields<'p>;

    // The shapes `serde_json` writes.
    fn is_human_readable(&self) -> bool {
        true
    }

    fn serialize_bool(self, v: bool) -> Result<Found, Error> {
        self.scalar(|| Ok(Value::Bool(v)))
    }

    fn serialize_i8(self, v: i8) -> Result<Found, Error> {
        self.scalar(|| Ok(Value::Int(i64::from(v))))
    }

    fn serialize_i16(self, v: i16) -> Result<Found, Error> {
        self.scalar(|| Ok(Value::Int(i64::from(v))))
    }

    fn serialize_i32(self, v: i32) -> Result<Found, Error> {
        self.scalar(|| Ok(Value::Int(i64::from(v))))
    }

    fn serialize_i64(self, v: i64) -> Result<Found, Error> {
        self.scalar(|| Ok(Value::Int(v)))
    }

    fn serialize_i128(self, v: i128) -> Result<Found, Error> {
        self.scalar(|| {
            if let Ok(n) = i64::try_from(v) {
                Ok(Value::Int(n))
            } else {
                u64::try_from(v)
                    .map(uint)
                    .map_err(|_| Error::msg("integer out of range for a rule value"))
            }
        })
    }

    fn serialize_u8(self, v: u8) -> Result<Found, Error> {
        self.scalar(|| Ok(Value::Int(i64::from(v))))
    }

    fn serialize_u16(self, v: u16) -> Result<Found, Error> {
        self.scalar(|| Ok(Value::Int(i64::from(v))))
    }

    fn serialize_u32(self, v: u32) -> Result<Found, Error> {
        self.scalar(|| Ok(Value::Int(i64::from(v))))
    }

    fn serialize_u64(self, v: u64) -> Result<Found, Error> {
        self.scalar(|| Ok(uint(v)))
    }

    fn serialize_u128(self, v: u128) -> Result<Found, Error> {
        self.scalar(|| {
            u64::try_from(v)
                .map(uint)
                .map_err(|_| Error::msg("integer out of range for a rule value"))
        })
    }

    fn serialize_f32(self, v: f32) -> Result<Found, Error> {
        self.scalar(|| Ok(Value::Float(f64::from(v))))
    }

    fn serialize_f64(self, v: f64) -> Result<Found, Error> {
        self.scalar(|| Ok(Value::Float(v)))
    }

    fn serialize_char(self, v: char) -> Result<Found, Error> {
        self.scalar(|| Ok(Value::String(v.to_string())))
    }

    fn serialize_str(self, v: &str) -> Result<Found, Error> {
        self.scalar(|| Ok(Value::String(v.to_owned())))
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<Found, Error> {
        self.scalar(|| Ok(Value::Bytes(v.to_vec())))
    }

    fn serialize_none(self) -> Result<Found, Error> {
        self.scalar(|| Ok(Value::Null))
    }

    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<Found, Error> {
        visit(value, self.path)
    }

    fn serialize_unit(self) -> Result<Found, Error> {
        self.scalar(|| Ok(Value::Null))
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Found, Error> {
        self.scalar(|| Ok(Value::Null))
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
    ) -> Result<Found, Error> {
        self.scalar(|| Ok(Value::String(variant.to_owned())))
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Found, Error> {
        visit(value, self.path)
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Found, Error> {
        match enter(self.path, variant) {
            Enter::Whole => Ok(Found::Opaque),
            Enter::Inside(rest) => visit(value, rest),
            Enter::Elsewhere => Ok(Found::Missing),
        }
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Items<'p>, Error> {
        Ok(Items::new(self.path))
    }

    fn serialize_tuple(self, _len: usize) -> Result<Items<'p>, Error> {
        Ok(Items::new(self.path))
    }

    fn serialize_tuple_struct(self, _name: &'static str, _len: usize) -> Result<Items<'p>, Error> {
        Ok(Items::new(self.path))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<Items<'p>, Error> {
        Ok(match enter(self.path, variant) {
            Enter::Whole => Items::Done(Found::Opaque),
            Enter::Inside(rest) => Items::new(rest),
            Enter::Elsewhere => Items::Done(Found::Missing),
        })
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Fields<'p>, Error> {
        Ok(Fields::new(self.path))
    }

    fn serialize_struct(self, name: &'static str, _len: usize) -> Result<Fields<'p>, Error> {
        // `serde_json`'s `arbitrary_precision` writes a number as a struct
        // holding its digits.
        if name == NUMBER_TOKEN && self.path.is_empty() {
            return Ok(Fields::Number(None));
        }
        Ok(Fields::new(self.path))
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<Fields<'p>, Error> {
        Ok(match enter(self.path, variant) {
            Enter::Whole => Fields::Done(Found::Opaque),
            Enter::Inside(rest) => Fields::new(rest),
            Enter::Elsewhere => Fields::Done(Found::Missing),
        })
    }
}

const NUMBER_TOKEN: &str = "$serde_json::private::Number";

/// Where `path` goes relative to an externally tagged variant, which serde
/// writes as the object `{variant: content}`.
enum Enter<'p> {
    /// The path is empty: the object itself.
    Whole,
    /// Into the content.
    Inside(&'p [Segment]),
    /// Another key: not this variant.
    Elsewhere,
}

fn enter<'p>(path: &'p [Segment], variant: &str) -> Enter<'p> {
    match path.split_first() {
        None => Enter::Whole,
        Some((Segment::Key { key, .. }, rest)) if key == variant => Enter::Inside(rest),
        Some(_) => Enter::Elsewhere,
    }
}

/// The items of a sequence, tuple, or tuple struct.
enum Items<'p> {
    /// Reading the whole array.
    Collect(Vec<Value>),
    /// Looking for the item at `index`; `found` is set once it is read.
    Find {
        index: usize,
        pos: usize,
        rest: &'p [Segment],
        found: Option<Found>,
    },
    /// The result is known; the items are not read.
    Done(Found),
}

impl<'p> Items<'p> {
    fn new(path: &'p [Segment]) -> Items<'p> {
        match path.split_first() {
            None => Items::Collect(Vec::new()),
            Some((&Segment::Index(index), rest)) => Items::Find {
                index,
                pos: 0,
                rest,
                found: None,
            },
            Some((Segment::Key { .. }, _)) => Items::Done(Found::Missing),
        }
    }

    fn push<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        match self {
            Items::Collect(items) => items.push(visit(value, &[])?.into_item()),
            Items::Find {
                index,
                pos,
                rest,
                found,
            } if found.is_none() => {
                if pos == index {
                    *found = Some(visit(value, rest)?);
                }
                *pos += 1;
            }
            Items::Find { .. } | Items::Done(_) => {}
        }
        Ok(())
    }

    fn finish(self) -> Result<Found, Error> {
        Ok(match self {
            Items::Collect(items) => Found::Value(Value::Array(items)),
            Items::Find { found, .. } => found.unwrap_or(Found::Missing),
            Items::Done(found) => found,
        })
    }
}

impl SerializeSeq for Items<'_> {
    type Ok = Found;
    type Error = Error;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        self.push(value)
    }

    fn end(self) -> Result<Found, Error> {
        self.finish()
    }
}

impl SerializeTuple for Items<'_> {
    type Ok = Found;
    type Error = Error;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        self.push(value)
    }

    fn end(self) -> Result<Found, Error> {
        self.finish()
    }
}

impl SerializeTupleStruct for Items<'_> {
    type Ok = Found;
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        self.push(value)
    }

    fn end(self) -> Result<Found, Error> {
        self.finish()
    }
}

impl SerializeTupleVariant for Items<'_> {
    type Ok = Found;
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        self.push(value)
    }

    fn end(self) -> Result<Found, Error> {
        self.finish()
    }
}

/// The entries of a map, struct, or struct variant.
enum Fields<'p> {
    /// Looking for `key`; `found` is set once its value is read. `matched`
    /// is a map key that matched, whose value is next.
    Find {
        key: &'p str,
        rest: &'p [Segment],
        matched: bool,
        found: Option<Found>,
    },
    /// The result is known; the entries are not read.
    Done(Found),
    /// A number written as a struct with its digits as the one field.
    Number(Option<Value>),
}

impl<'p> Fields<'p> {
    fn new(path: &'p [Segment]) -> Fields<'p> {
        match path.split_first() {
            None => Fields::Done(Found::Opaque),
            Some((Segment::Key { key, .. }, rest)) => Fields::Find {
                key,
                rest,
                matched: false,
                found: None,
            },
            Some((Segment::Index(_), _)) => Fields::Done(Found::Missing),
        }
    }

    fn field<T: ?Sized + Serialize>(&mut self, name: &str, value: &T) -> Result<(), Error> {
        match self {
            Fields::Find {
                key, rest, found, ..
            } if found.is_none() && *key == name => *found = Some(visit(value, rest)?),
            Fields::Number(digits) => *digits = Some(value.serialize(Text(number))??),
            Fields::Find { .. } | Fields::Done(_) => {}
        }
        Ok(())
    }

    fn key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<(), Error> {
        if let Fields::Find {
            key: want,
            matched,
            found: None,
            ..
        } = self
        {
            *matched = key.serialize(Text(|text: &str| text == *want))?;
        }
        Ok(())
    }

    fn value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        if let Fields::Find {
            rest,
            matched: matched @ true,
            found,
            ..
        } = self
        {
            *matched = false;
            *found = Some(visit(value, rest)?);
        }
        Ok(())
    }

    fn finish(self) -> Result<Found, Error> {
        Ok(match self {
            Fields::Find { found, .. } => found.unwrap_or(Found::Missing),
            Fields::Done(found) => found,
            Fields::Number(digits) => digits.map_or(Found::Missing, Found::Value),
        })
    }
}

fn number(text: &str) -> Result<Value, Error> {
    if let Ok(n) = text.parse::<i64>() {
        Ok(Value::Int(n))
    } else if let Ok(n) = text.parse::<u64>() {
        Ok(Value::Uint(n))
    } else if let Ok(n) = text.parse::<f64>() {
        Ok(Value::Float(n))
    } else {
        Err(Error(format!("invalid number {text:?}")))
    }
}

impl SerializeMap for Fields<'_> {
    type Ok = Found;
    type Error = Error;

    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<(), Error> {
        self.key(key)
    }

    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        self.value(value)
    }

    fn end(self) -> Result<Found, Error> {
        self.finish()
    }
}

impl SerializeStruct for Fields<'_> {
    type Ok = Found;
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        self.field(key, value)
    }

    fn end(self) -> Result<Found, Error> {
        self.finish()
    }
}

impl SerializeStructVariant for Fields<'_> {
    type Ok = Found;
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        self.field(key, value)
    }

    fn end(self) -> Result<Found, Error> {
        self.finish()
    }
}

/// Serializes a map key or a number's digits: hands `f` the text of a
/// string, `char`, `bool`, integer, unit variant name, or a newtype around
/// one of those, as `serde_json` writes a map key. Anything else is an
/// error.
struct Text<F>(F);

fn not_text<T>() -> Result<T, Error> {
    Err(Error::msg("map key must be a string"))
}

macro_rules! text_of_display {
    ($($method:ident($ty:ty)),* $(,)?) => {$(
        fn $method(self, v: $ty) -> Result<R, Error> {
            Ok((self.0)(&v.to_string()))
        }
    )*};
}

impl<F, R> Serializer for Text<F>
where
    F: FnOnce(&str) -> R,
{
    type Ok = R;
    type Error = Error;
    type SerializeSeq = Impossible<R, Error>;
    type SerializeTuple = Impossible<R, Error>;
    type SerializeTupleStruct = Impossible<R, Error>;
    type SerializeTupleVariant = Impossible<R, Error>;
    type SerializeMap = Impossible<R, Error>;
    type SerializeStruct = Impossible<R, Error>;
    type SerializeStructVariant = Impossible<R, Error>;

    text_of_display! {
        serialize_i8(i8), serialize_i16(i16), serialize_i32(i32), serialize_i64(i64),
        serialize_i128(i128), serialize_u8(u8), serialize_u16(u16), serialize_u32(u32),
        serialize_u64(u64), serialize_u128(u128),
    }

    fn serialize_bool(self, v: bool) -> Result<R, Error> {
        Ok((self.0)(if v { "true" } else { "false" }))
    }

    fn serialize_char(self, v: char) -> Result<R, Error> {
        Ok((self.0)(v.encode_utf8(&mut [0; 4])))
    }

    fn serialize_str(self, v: &str) -> Result<R, Error> {
        Ok((self.0)(v))
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
    ) -> Result<R, Error> {
        Ok((self.0)(variant))
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<R, Error> {
        value.serialize(self)
    }

    fn serialize_f32(self, _v: f32) -> Result<R, Error> {
        not_text()
    }

    fn serialize_f64(self, _v: f64) -> Result<R, Error> {
        not_text()
    }

    fn serialize_bytes(self, _v: &[u8]) -> Result<R, Error> {
        not_text()
    }

    fn serialize_none(self) -> Result<R, Error> {
        not_text()
    }

    fn serialize_some<T: ?Sized + Serialize>(self, _value: &T) -> Result<R, Error> {
        not_text()
    }

    fn serialize_unit(self) -> Result<R, Error> {
        not_text()
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<R, Error> {
        not_text()
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _index: u32,
        _variant: &'static str,
        _value: &T,
    ) -> Result<R, Error> {
        not_text()
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Impossible<R, Error>, Error> {
        not_text()
    }

    fn serialize_tuple(self, _len: usize) -> Result<Impossible<R, Error>, Error> {
        not_text()
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Impossible<R, Error>, Error> {
        not_text()
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Impossible<R, Error>, Error> {
        not_text()
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Impossible<R, Error>, Error> {
        not_text()
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Impossible<R, Error>, Error> {
        not_text()
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Impossible<R, Error>, Error> {
        not_text()
    }
}
