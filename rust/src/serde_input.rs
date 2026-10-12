//! [`SerdeInput`]: read any [`serde::Serialize`] value as an input.
//!
//! A path is resolved by running `T::serialize` into a serializer that
//! follows it. Only what the path names is serialized, and the walk stops at
//! the target; the exceptions are listed under "What is serialized" on
//! [`SerdeInput`].

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
/// one walk through `T::serialize`, stopped at the target, and the value read
/// is owned (serde lends a string for one callback only).
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
/// - `NaN` and the infinities are `null`, as in `serde_json::to_value`. A
///   map key may also be a finite float, written as `serde_json` writes it.
/// - A `serde_json::value::RawValue` is its JSON, parsed, and the path
///   continues into it.
///
/// # What is serialized
///
/// Only what the path names. A struct field, map value, variant, or list
/// item off the path never has its `serialize` called, and the walk stops
/// when the target is read, so nothing after it is visited: a later field
/// that fails to serialize does not matter. Three things still run before
/// the target:
///
/// - A map key is serialized, to compare it with the path.
/// - Your own `Serialize` code, `skip_serializing_if` predicates included,
///   runs as written up to the target.
/// - Serde itself buffers the payload of a `#[serde(flatten)]`ed externally
///   tagged enum's struct or tuple variant, in full, before rulekit sees it.
///   Its fields are serialized even when the path names a different one, and
///   a field that fails there is an input error.
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
/// Serde buffers the payload of a `#[serde(flatten)]`ed externally tagged
/// struct or tuple variant (see [What is serialized](#what-is-serialized)),
/// so a typed value inside one reads as a string too.
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
        Ok(resolve(self.0, path)?.into_val())
    }
}

impl<T: ?Sized + Serialize> Input for SerdeInput<'_, T> {
    fn get<'a>(&'a self, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        InputValue::get(self, path)
    }
}

/// What a walk found at the end of the path.
#[derive(Debug)]
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
/// [`downcast`] relies on every implementor being a concrete type whose
/// expanded form has no lifetimes at all: not a lifetime parameter, not an
/// alias for `Foo<'static>`, not a reference or function pointer. `'static`
/// alone is not enough: `Foo<'static>` and `Foo<'short>` share a `typeid`.
/// The trait is private, so only the list below implements it; adding to
/// that list needs this check and the `downcast_tests` matrix.
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
    // its free lifetimes erased (typeid 1.x documents non-`'static` support;
    // higher-ranked binders such as `for<'a> fn(&'a str)` stay distinct).
    // `U: Leaf` contains no lifetimes (the contract of `Leaf`), so an id
    // equal to that of `U`, `&U`, or `&&U` means `T` is `U` or a shared
    // reference chain to it, whatever the lifetimes of those references;
    // `&mut` is a different id. `ptr` is valid and aligned for `T` and lives
    // as long as `value`, so each pointer cast and dereference reads the `U`
    // that `value` holds or points to. The returned borrow lasts no longer
    // than `value`'s own, which every reference in `T` outlives.
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
        Err(err) => Err(Error::msg(&err.to_string())),
    }
}

/// Why a walk stopped.
#[derive(Debug)]
enum Error {
    /// The path resolved to this. It is returned as an error so that the
    /// `Serialize` impls on the way out stop too: nothing after the target is
    /// serialized, not even what serde itself buffers for a flattened enum.
    /// [`resolve`] turns it back into the value.
    Done(Found),
    /// A real failure: a `Serialize` impl's error, or a value a rule cannot
    /// hold.
    Failed(String),
}

impl Error {
    fn msg(msg: &str) -> Error {
        Error::Failed(msg.to_owned())
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Done(_) => f.write_str("lookup finished (a `Serialize` impl replaced this)"),
            Error::Failed(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for Error {}

impl ser::Error for Error {
    fn custom<T: Display>(msg: T) -> Error {
        Error::Failed(msg.to_string())
    }
}

/// [`visit`], with the early exit of a resolved path turned back into its
/// value: what a walk from the root, or into a value of its own, returns.
fn resolve<T: ?Sized + Serialize>(value: &T, path: &[Segment]) -> Result<Found, Error> {
    match visit(value, path) {
        Err(Error::Done(found)) => Ok(found),
        result => result,
    }
}

/// The unsigned integer a rule sees: `int64` when it fits.
fn uint(n: u64) -> Value {
    i64::try_from(n).map_or(Value::Uint(n), Value::Int)
}

/// A float as `serde_json::to_value` reads it: `NaN` and the infinities are
/// `null`.
fn float(v: f64) -> Value {
    if v.is_finite() {
        Value::Float(v)
    } else {
        Value::Null
    }
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
        self.scalar(|| Ok(float(f64::from(v))))
    }

    fn serialize_f64(self, v: f64) -> Result<Found, Error> {
        self.scalar(|| Ok(float(v)))
    }

    fn serialize_char(self, v: char) -> Result<Found, Error> {
        self.scalar(|| Ok(Value::String(v.to_string())))
    }

    // Formats once, into the string the value owns, and not at all when a
    // path below a string is read.
    fn collect_str<T: ?Sized + Display>(self, value: &T) -> Result<Found, Error> {
        self.scalar(|| Ok(Value::String(value.to_string())))
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
        Ok(match name {
            NUMBER_TOKEN if self.path.is_empty() => Fields::Number(None),
            // Nothing is below a number.
            NUMBER_TOKEN => Fields::Done(Found::Missing),
            RAW_TOKEN => Fields::Raw {
                path: self.path,
                out: None,
            },
            _ => Fields::new(self.path),
        })
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

// serde_json writes a number (feature `arbitrary_precision`) and a `RawValue`
// (feature `raw_value`) as a struct with one field, named like the struct,
// that holds the digits or the JSON text.
const NUMBER_TOKEN: &str = "$serde_json::private::Number";
const RAW_TOKEN: &str = "$serde_json::private::RawValue";

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
    /// Looking for the item at `index`, which ends the walk.
    Find {
        index: usize,
        pos: usize,
        rest: &'p [Segment],
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
            },
            Some((Segment::Key { .. }, _)) => Items::Done(Found::Missing),
        }
    }

    fn push<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        match self {
            Items::Collect(items) => items.push(visit(value, &[])?.into_item()),
            Items::Find { index, pos, rest } => {
                if pos == index {
                    return Err(Error::Done(visit(value, rest)?));
                }
                *pos += 1;
            }
            Items::Done(_) => {}
        }
        Ok(())
    }

    fn finish(self) -> Result<Found, Error> {
        Ok(match self {
            Items::Collect(items) => Found::Value(Value::Array(items)),
            // Past the end.
            Items::Find { .. } => Found::Missing,
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
    /// Looking for `key`. `matched` is a map key that matched, whose value is
    /// next. The value ends the walk, so only the first of two equal keys is
    /// read.
    Find {
        key: &'p str,
        rest: &'p [Segment],
        matched: bool,
    },
    /// The result is known; the entries are not read.
    Done(Found),
    /// A number written as a struct with its digits as the one field.
    Number(Option<Value>),
    /// A `RawValue`: a struct with its JSON text as the one field.
    Raw {
        path: &'p [Segment],
        out: Option<Found>,
    },
}

impl<'p> Fields<'p> {
    fn new(path: &'p [Segment]) -> Fields<'p> {
        match path.split_first() {
            None => Fields::Done(Found::Opaque),
            Some((Segment::Key { key, .. }, rest)) => Fields::Find {
                key,
                rest,
                matched: false,
            },
            Some((Segment::Index(_), _)) => Fields::Done(Found::Missing),
        }
    }

    fn field<T: ?Sized + Serialize>(&mut self, name: &str, value: &T) -> Result<(), Error> {
        match self {
            Fields::Find { key, rest, .. } if *key == name => Err(Error::Done(visit(value, rest)?)),
            Fields::Number(digits) => {
                *digits = Some(value.serialize(Text(number))??);
                Ok(())
            }
            Fields::Raw { path, out } => {
                let path = *path;
                *out = Some(value.serialize(Text(|text: &str| raw(text, path)))??);
                Ok(())
            }
            Fields::Find { .. } | Fields::Done(_) => Ok(()),
        }
    }

    fn key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<(), Error> {
        if let Fields::Find {
            key: want, matched, ..
        } = self
        {
            *matched = key.serialize(Text(|text: &str| text == *want))?;
        }
        Ok(())
    }

    fn value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        match self {
            Fields::Find {
                rest,
                matched: true,
                ..
            } => Err(Error::Done(visit(value, rest)?)),
            _ => Ok(()),
        }
    }

    fn finish(self) -> Result<Found, Error> {
        Ok(match self {
            // The key never came up.
            Fields::Find { .. } => Found::Missing,
            Fields::Done(found) => found,
            Fields::Number(digits) => digits.map_or(Found::Missing, Found::Value),
            Fields::Raw { out, .. } => out.unwrap_or(Found::Missing),
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
        Err(Error::msg(&format!("invalid number {text:?}")))
    }
}

/// What `serde_json::to_value` makes of a `RawValue`: its JSON text, parsed.
/// The path continues into that value, walked like any other `Serialize`.
fn raw(text: &str, path: &[Segment]) -> Result<Found, Error> {
    let parsed: serde_json::Value =
        serde_json::from_str(text).map_err(|err| Error::msg(&err.to_string()))?;
    resolve(&parsed, path)
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
/// string, `char`, `bool`, integer, finite float, unit variant name, or a
/// newtype around one of those, as `serde_json` writes a map key. Anything
/// else is an error.
struct Text<F>(F);

fn not_text<T>() -> Result<T, Error> {
    Err(Error::msg("map key must be a string"))
}

// The text `serde_json` writes for a finite float, which it also uses for a
// map key. `NaN` and the infinities are an error there, as here.
macro_rules! text_of_float {
    ($($method:ident($ty:ty)),* $(,)?) => {$(
        fn $method(self, v: $ty) -> Result<R, Error> {
            if !v.is_finite() {
                return Err(Error::msg("float key must be finite (got NaN or +/-inf)"));
            }
            let text = serde_json::to_string(&v).map_err(|err| Error::msg(&err.to_string()))?;
            Ok((self.0)(&text))
        }
    )*};
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

    text_of_float! { serialize_f32(f32), serialize_f64(f64) }

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

/// The one `unsafe` block in this crate is [`downcast`]. These tests pin its
/// contract directly, and run under Miri (`make miri-rust`, both aliasing
/// models) to check that every reference it returns is valid for the borrow
/// it is handed out with.
#[cfg(test)]
mod downcast_tests {
    use super::*;
    use std::borrow::Cow;
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::Arc;

    /// Runs `$body` with `$v` bound to one value of each leaf type, `$t`
    /// being its type, including a three-byte OUI `Mac` and both URL types.
    macro_rules! each_leaf {
        ($m:ident) => {
            $m! {
                a: IpAddr = "10.1.2.3".parse().unwrap(),
                a6: IpAddr = "::ffff:10.1.2.3".parse().unwrap(),
                b: Ipv4Addr = "192.168.0.1".parse().unwrap(),
                c: Ipv6Addr = "fe80::1".parse().unwrap(),
                d: IpNet = "10.0.0.0/8".parse().unwrap(),
                e: Ipv4Net = "0.0.0.0/0".parse().unwrap(),
                f: Ipv6Net = "2001:db8::/128".parse().unwrap(),
                g: Ip = Ip::parse("1.2.3.4").unwrap(),
                h: Cidr = Cidr::parse("fd00::/8").unwrap(),
                i: Mac = Mac::parse("aa:bb:cc:dd:ee:ff").unwrap(),
                i8: Mac = Mac::parse("aa:bb:cc:dd:ee:ff:00:11").unwrap(),
                oui: Mac = Mac::parse("aa:bb:cc:dd:ee:ff").unwrap().oui(),
                j: Url = Url::parse("https://user@example.com:8443/a/b?q=1&q=2#f").unwrap(),
                j2: Url = Url::parse("/relative/only").unwrap(),
                #[cfg(feature = "url")]
                k: ::url::Url = ::url::Url::parse("http://example.org/x?y=%2F").unwrap(),
            }
        };
    }

    /// `downcast` of `U`, `&U`, and `&&U` returns the very `U` it was given,
    /// not a copy and not a neighbour; every other shape returns `None`.
    fn check<U: Leaf + Clone>(value: U) {
        assert!(std::ptr::eq(downcast::<U, U>(&value).unwrap(), &value));
        let one = &value;
        assert!(std::ptr::eq(downcast::<&U, U>(&one).unwrap(), &value));
        let two = &one;
        assert!(std::ptr::eq(downcast::<&&U, U>(&two).unwrap(), &value));
        let three = &two;
        assert!(downcast::<&&&U, U>(&three).is_none());
        assert!(downcast::<&&&&U, U>(&&three).is_none());

        let mut owned = value.clone();
        {
            let mutable = &mut owned;
            assert!(downcast::<&mut U, U>(&mutable).is_none());
            assert!(downcast::<&&mut U, U>(&&mutable).is_none());
        }
        {
            let shared = &value;
            let mut slot = shared;
            let mixed = &mut slot;
            assert!(downcast::<&mut &U, U>(&mixed).is_none());
        }
        let raw: *const U = &value;
        assert!(downcast::<*const U, U>(&raw).is_none());
        assert!(downcast::<*mut U, U>(&(raw as *mut U)).is_none());
        assert!(downcast::<Box<U>, U>(&Box::new(value.clone())).is_none());
        assert!(downcast::<&Box<U>, U>(&&Box::new(value.clone())).is_none());
        assert!(downcast::<Rc<U>, U>(&Rc::new(value.clone())).is_none());
        assert!(downcast::<Arc<U>, U>(&Arc::new(value.clone())).is_none());
        assert!(downcast::<Option<U>, U>(&Some(value.clone())).is_none());
        assert!(downcast::<Option<&U>, U>(&Some(&value)).is_none());
        assert!(downcast::<Cow<'_, U>, U>(&Cow::Borrowed(&value)).is_none());
        assert!(downcast::<Cell<&U>, U>(&Cell::new(&value)).is_none());
        let array: [U; 1] = [value.clone()];
        assert!(downcast::<[U; 1], U>(&array).is_none());
        assert!(downcast::<(U,), U>(&(value.clone(),)).is_none());
        assert!(downcast::<[U], U>(std::slice::from_ref(&value)).is_none());
        assert!(downcast::<[U], U>(&[] as &[U]).is_none());
        assert!(downcast::<&[U], U>(&std::slice::from_ref(&value)).is_none());
        let erased: &dyn Erased = &value;
        assert!(downcast::<dyn Erased, U>(erased).is_none());
        assert!(downcast::<&dyn Erased, U>(&erased).is_none());

        /// The same layout as `U`, a different type.
        #[repr(transparent)]
        struct Lookalike<U>(U);
        assert!(downcast::<Lookalike<U>, U>(&Lookalike(value.clone())).is_none());

        /// A lifetime-bearing wrapper around a borrowed `U`.
        struct Borrowed<'a, U>(#[allow(dead_code)] &'a U);
        assert!(downcast::<Borrowed<'_, U>, U>(&Borrowed(&value)).is_none());
    }

    trait Erased {}
    impl<T: ?Sized> Erased for T {}

    #[test]
    fn each_leaf_by_value_and_through_one_or_two_references() {
        macro_rules! run {
            ($($(#[$cfg:meta])* $v:ident : $t:ty = $e:expr),* $(,)?) => {
                $($(#[$cfg])* check::<$t>($e);)*
            };
        }
        each_leaf!(run);
    }

    /// No leaf matches another at any accepted depth, including the pairs
    /// with the same layout (`Ip` wraps an `IpAddr`, `Cidr` an `IpNet`).
    #[test]
    fn leaves_do_not_alias_each_other() {
        macro_rules! cross {
            ($($(#[$cfg:meta])* $v:ident : $t:ty = $e:expr),* $(,)?) => {
                $($(#[$cfg])* let $v: $t = $e;)*
                cross!(@all [$($(#[$cfg])* $t),*] $($(#[$cfg])* $v : $t),*);
            };
            (@all $list:tt $($(#[$cfg:meta])* $v:ident : $t:ty),*) => {
                $($(#[$cfg])* cross!(@one $v : $t ; $list);)*
            };
            (@one $v:ident : $t:ty ; [$($(#[$cfg:meta])* $u:ty),*]) => {
                $(
                    $(#[$cfg])*
                    if typeid::of::<$t>() != typeid::of::<$u>() {
                        assert!(downcast::<$t, $u>(&$v).is_none());
                        assert!(downcast::<&$t, $u>(&&$v).is_none());
                        assert!(downcast::<&&$t, $u>(&&&$v).is_none());
                    }
                )*
            };
        }
        each_leaf!(cross);
        assert!(downcast::<str, Url>("https://example.com").is_none());
        assert!(downcast::<&str, Url>(&"https://example.com").is_none());
        assert!(downcast::<String, Url>(&"https://example.com".to_owned()).is_none());
        assert!(downcast::<[u8; 6], Mac>(&[1, 2, 3, 4, 5, 6]).is_none());
        assert!(downcast::<(), Mac>(&()).is_none());
    }

    /// Higher-ranked types keep their binders in `typeid`, and none of them
    /// is a leaf.
    #[test]
    fn higher_ranked_types_are_not_leaves() {
        fn takes(_: &str) {}
        let hr: for<'a> fn(&'a str) = takes;
        let st: fn(&'static str) = takes;
        assert_ne!(
            typeid::of::<for<'a> fn(&'a str)>(),
            typeid::of::<fn(&'static str)>()
        );
        assert!(downcast::<for<'a> fn(&'a str), Url>(&hr).is_none());
        assert!(downcast::<fn(&'static str), Url>(&st).is_none());
        let obj: &dyn for<'a> Fn(&'a IpAddr) = &|_| {};
        assert!(downcast::<dyn for<'a> Fn(&'a IpAddr), IpAddr>(obj).is_none());
    }

    /// References with short, distinct lifetimes still match: `typeid`
    /// erases them, and the borrow returned lives no longer than the input.
    /// Under Miri, mutating the leaf after the borrow ends checks that the
    /// cast left no live alias behind.
    #[test]
    fn short_lived_references() {
        fn inner<'a, 'b>(value: &'a &'b IpAddr) -> Option<&'a IpAddr> {
            downcast::<&'b IpAddr, IpAddr>(value)
        }
        fn inner2<'a, 'b, 'c>(value: &'a &'b &'c Url) -> Option<&'a Url> {
            downcast::<&'b &'c Url, Url>(value)
        }
        for n in 0..4u8 {
            let mut addr = IpAddr::from([10, 0, 0, n]);
            {
                let borrowed = &addr;
                let got = inner(&borrowed).unwrap();
                assert!(std::ptr::eq(got, &addr));
                assert_eq!(*got, IpAddr::from([10, 0, 0, n]));
            }
            addr = IpAddr::from([0, 0, 0, 0]);
            assert!(addr.is_unspecified());

            let mut url = Url::parse(&format!("https://h{n}.example/p")).unwrap();
            {
                let one = &url;
                let two = &one;
                let got = inner2(&two).unwrap();
                assert!(std::ptr::eq(got, &url));
            }
            url = Url::parse("https://moved.example").unwrap();
            drop(url);
        }
    }

    /// `typed` recognises each leaf as `T`, `&T`, and `&&T`, and its read is
    /// the leaf's own `InputValue` read, compared as values, not as text.
    #[test]
    fn typed_reads_each_leaf_as_its_own_input_value() {
        fn own<U: Leaf>(value: &U, path: &[Segment]) -> Option<Value> {
            InputValue::get(value, path)
                .unwrap()
                .map(|val| Found::from_val(val).into_item())
        }
        fn via(found: Result<Found, Error>) -> Option<Value> {
            match found.unwrap() {
                Found::Missing => None,
                found => Some(found.into_item()),
            }
        }
        fn same<U: Leaf>(value: &U) {
            let key = |k: &str| Segment::Key {
                key: k.to_owned(),
                bracket: false,
            };
            let mut paths = vec![vec![], vec![Segment::Index(0)]];
            for f in [
                "version", "ip", "prefix", "host", "port", "scheme", "path", "query", "user",
                "oui", "fragment", "zz",
            ] {
                paths.push(vec![key(f)]);
                paths.push(vec![key(f), key("q")]);
                paths.push(vec![key(f), Segment::Index(0)]);
            }
            for path in &paths {
                let want = own(value, path);
                assert_eq!(via(typed(value, path).unwrap()), want, "{path:?}");
                assert_eq!(via(typed(&value, path).unwrap()), want, "{path:?}");
                assert_eq!(via(typed(&&value, path).unwrap()), want, "{path:?}");
                assert!(typed(&&&value, path).is_none());
            }
        }
        macro_rules! run {
            ($($(#[$cfg:meta])* $v:ident : $t:ty = $e:expr),* $(,)?) => {
                $($(#[$cfg])* same::<$t>(&$e);)*
            };
        }
        each_leaf!(run);
    }

    /// A `Serialize` impl may hand the walk a leaf it builds on its own
    /// stack, dropped before the read returns: the read must own its result.
    #[test]
    fn leaves_built_inside_serialize_are_read_before_they_drop() {
        struct Temp;
        impl Serialize for Temp {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                use serde::ser::SerializeStruct;
                let url = Url::parse("https://tmp.example:9/a?k=v").unwrap();
                let one = &url;
                let ip = IpAddr::from([127, 0, 0, 1]);
                let mut st = s.serialize_struct("Temp", 2)?;
                st.serialize_field("url", &one)?;
                st.serialize_field("ip", &ip)?;
                st.end()
            }
        }
        let key = |k: &str| Segment::Key {
            key: k.to_owned(),
            bracket: false,
        };
        let host = resolve(&Temp, &[key("url"), key("host")])
            .unwrap()
            .into_item();
        assert_eq!(host, Value::String("tmp.example".into()));
        let whole = resolve(&Temp, &[key("url")]).unwrap().into_item();
        assert_eq!(
            whole,
            own_url("https://tmp.example:9/a?k=v"),
            "the whole URL outlives the `Url` it was read from"
        );
        let ip = resolve(&Temp, &[key("ip")]).unwrap().into_item();
        assert_eq!(
            ip,
            Found::from_val(
                InputValue::get(&IpAddr::from([127, 0, 0, 1]), &[])
                    .unwrap()
                    .unwrap()
            )
            .into_item()
        );

        fn own_url(s: &str) -> Value {
            let url = Url::parse(s).unwrap();
            Found::from_val(InputValue::get(&url, &[]).unwrap().unwrap()).into_item()
        }
    }
}
