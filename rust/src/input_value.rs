//! [`InputValue`]: plain Rust values a rule can read without converting them
//! up front.

use std::borrow::{Borrow, Cow};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::hash::{BuildHasher, Hash};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::rc::Rc;
use std::sync::Arc;

use ipnet::{IpNet, Ipv4Net, Ipv6Net};

use crate::ast::Segment;
use crate::error::BoxError;
use crate::input::Input;
use crate::value::{
    ArrayRef, Cidr, Ip, ListSource, Mac, ObjectRef, ObjectSource, Url, Val, Value, ValueRef,
    value_field,
};

/// A value a rule can read: a derive field, a `kv!` entry, or a nested lookup.
///
/// `get` resolves `path` relative to this value. An empty path is the value
/// itself. `Ok(None)` means the path is absent (a missing field), which is
/// what [`Option::None`] produces. Only the segments in `path` are read.
///
/// # Bytes
///
/// `Vec<u8>`, `&[u8]`, and `[u8; N]` are lists of numbers, not byte strings.
/// A byte string and a list of `u8` are the same Rust type, so the list impl
/// wins. Pass [`Value::Bytes`] when the rule should see bytes.
///
/// # Errors
///
/// A caller-defined value (a lazy field, a custom impl) may fail. The impls
/// in this crate for plain data do not.
pub trait InputValue<C: ?Sized = ()> {
    /// The value at `path`, borrowing from `self` or `ctx` when it can.
    fn get<'a>(&'a self, ctx: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError>;
}

/// Walk `path` from a value already in hand.
pub(crate) fn project<'a>(val: Val<'a>, path: &[Segment]) -> Option<Val<'a>> {
    let Some((head, rest)) = path.split_first() else {
        return Some(val);
    };
    project(step(val, head)?, rest)
}

fn step<'a>(val: Val<'a>, seg: &Segment) -> Option<Val<'a>> {
    match seg {
        Segment::Index(index) => match val {
            Val::Ref(ValueRef::Array(items)) => items.get(*index).map(Val::Ref),
            Val::Owned(Value::Array(mut items)) if *index < items.len() => {
                Some(Val::Owned(items.swap_remove(*index)))
            }
            _ => None,
        },
        Segment::Key { key, .. } => match val {
            Val::Ref(ValueRef::Object(ObjectRef::Map(map))) => {
                map.get(key).map(|v| Val::Ref(v.as_ref()))
            }
            Val::Ref(ValueRef::Object(ObjectRef::Source(src))) => src.get(key),
            Val::Ref(ValueRef::Object(ObjectRef::Opaque)) => None,
            Val::Ref(v) => value_field(v, key),
            Val::Owned(Value::Object(mut map)) => map.remove(key).map(Val::Owned),
            Val::Owned(v) => value_field(v.as_ref(), key).map(|f| Val::Owned(f.into_owned())),
        },
    }
}

fn leaf<'a>(value: ValueRef<'a>, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
    Ok(project(Val::Ref(value), path))
}

// Sized only: `&[T]` is its own impl so a slice field can hand out
// `&&[T]` (sized) as `&dyn ListSource`. A `?Sized` blanket would overlap it.
impl<C: ?Sized, T: InputValue<C>> InputValue<C> for &T {
    fn get<'a>(&'a self, ctx: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        InputValue::get(&**self, ctx, path)
    }
}

impl<C: ?Sized, T: InputValue<C> + ?Sized> InputValue<C> for Box<T> {
    fn get<'a>(&'a self, ctx: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        InputValue::get(&**self, ctx, path)
    }
}

impl<C: ?Sized, T: InputValue<C> + ?Sized> InputValue<C> for Rc<T> {
    fn get<'a>(&'a self, ctx: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        InputValue::get(&**self, ctx, path)
    }
}

impl<C: ?Sized, T: InputValue<C> + ?Sized> InputValue<C> for Arc<T> {
    fn get<'a>(&'a self, ctx: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        InputValue::get(&**self, ctx, path)
    }
}

impl<C: ?Sized, T: InputValue<C>> InputValue<C> for Option<T> {
    fn get<'a>(&'a self, ctx: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        match self {
            Some(value) => InputValue::get(value, ctx, path),
            None => Ok(None),
        }
    }
}

impl<C: ?Sized> InputValue<C> for bool {
    fn get<'a>(&'a self, _: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        leaf(ValueRef::Bool(*self), path)
    }
}

macro_rules! signed {
    ($($t:ty),*) => {$(
        impl<C: ?Sized> InputValue<C> for $t {
            fn get<'a>(&'a self, _: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
                leaf(ValueRef::Int(i64::from(*self)), path)
            }
        }
    )*};
}
signed!(i8, i16, i32);

impl<C: ?Sized> InputValue<C> for i64 {
    fn get<'a>(&'a self, _: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        leaf(ValueRef::Int(*self), path)
    }
}

impl<C: ?Sized> InputValue<C> for isize {
    fn get<'a>(&'a self, _: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        // `isize` fits in `i64` on every supported target.
        leaf(ValueRef::Int(*self as i64), path)
    }
}

macro_rules! small_unsigned {
    ($($t:ty),*) => {$(
        impl<C: ?Sized> InputValue<C> for $t {
            fn get<'a>(&'a self, _: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
                leaf(ValueRef::Int(i64::from(*self)), path)
            }
        }
    )*};
}
small_unsigned!(u8, u16, u32);

fn wide_uint(n: u64) -> ValueRef<'static> {
    if n <= i64::MAX as u64 {
        ValueRef::Int(n as i64)
    } else {
        ValueRef::Uint(n)
    }
}

impl<C: ?Sized> InputValue<C> for u64 {
    fn get<'a>(&'a self, _: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        leaf(wide_uint(*self), path)
    }
}

impl<C: ?Sized> InputValue<C> for usize {
    fn get<'a>(&'a self, _: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        leaf(wide_uint(*self as u64), path)
    }
}

impl<C: ?Sized> InputValue<C> for f32 {
    fn get<'a>(&'a self, _: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        leaf(ValueRef::Float(f64::from(*self)), path)
    }
}

impl<C: ?Sized> InputValue<C> for f64 {
    fn get<'a>(&'a self, _: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        leaf(ValueRef::Float(*self), path)
    }
}

impl<C: ?Sized> InputValue<C> for str {
    fn get<'a>(&'a self, _: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        leaf(ValueRef::Str(self), path)
    }
}

impl<C: ?Sized> InputValue<C> for &str {
    fn get<'a>(&'a self, ctx: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        InputValue::get(*self, ctx, path)
    }
}

impl<C: ?Sized> InputValue<C> for String {
    fn get<'a>(&'a self, ctx: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        InputValue::get(self.as_str(), ctx, path)
    }
}

impl<C: ?Sized> InputValue<C> for Cow<'_, str> {
    fn get<'a>(&'a self, ctx: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        InputValue::get(self.as_ref(), ctx, path)
    }
}

impl<C: ?Sized> InputValue<C> for IpAddr {
    fn get<'a>(&'a self, _: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        leaf(ValueRef::Ip(Ip::from_addr(*self)), path)
    }
}

impl<C: ?Sized> InputValue<C> for Ipv4Addr {
    fn get<'a>(&'a self, _: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        leaf(ValueRef::Ip(Ip::from_addr(IpAddr::V4(*self))), path)
    }
}

impl<C: ?Sized> InputValue<C> for Ipv6Addr {
    fn get<'a>(&'a self, _: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        leaf(ValueRef::Ip(Ip::from_addr(IpAddr::V6(*self))), path)
    }
}

impl<C: ?Sized> InputValue<C> for Ip {
    fn get<'a>(&'a self, _: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        leaf(ValueRef::Ip(*self), path)
    }
}

impl<C: ?Sized> InputValue<C> for IpNet {
    fn get<'a>(&'a self, _: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        leaf(ValueRef::Cidr(Cidr::from_net(*self)), path)
    }
}

impl<C: ?Sized> InputValue<C> for Ipv4Net {
    fn get<'a>(&'a self, _: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        leaf(ValueRef::Cidr(Cidr::from_net(IpNet::from(*self))), path)
    }
}

impl<C: ?Sized> InputValue<C> for Ipv6Net {
    fn get<'a>(&'a self, _: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        leaf(ValueRef::Cidr(Cidr::from_net(IpNet::from(*self))), path)
    }
}

impl<C: ?Sized> InputValue<C> for Cidr {
    fn get<'a>(&'a self, _: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        leaf(ValueRef::Cidr(*self), path)
    }
}

impl<C: ?Sized> InputValue<C> for Mac {
    fn get<'a>(&'a self, _: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        leaf(ValueRef::Mac(*self), path)
    }
}

impl<C: ?Sized> InputValue<C> for Url {
    fn get<'a>(&'a self, _: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        leaf(ValueRef::Url(self), path)
    }
}

impl<C: ?Sized> InputValue<C> for Value {
    fn get<'a>(&'a self, _: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        leaf(self.as_ref(), path)
    }
}

fn view_item<T: InputValue<()>>(item: &T) -> ValueRef<'_> {
    match InputValue::get(item, &(), &[]) {
        Ok(Some(Val::Ref(value))) => value,
        // A present item that cannot be borrowed is null, so list length holds.
        _ => ValueRef::Null,
    }
}

impl<T> ListSource for &[T]
where
    T: InputValue<()>,
{
    fn len(&self) -> usize {
        (*self).len()
    }

    fn get(&self, index: usize) -> Option<ValueRef<'_>> {
        <[T]>::get(self, index).map(view_item)
    }
}

impl<T> ListSource for Vec<T>
where
    T: InputValue<()>,
{
    fn len(&self) -> usize {
        Vec::len(self)
    }

    fn get(&self, index: usize) -> Option<ValueRef<'_>> {
        self.as_slice().get(index).map(view_item)
    }
}

impl<T, const N: usize> ListSource for [T; N]
where
    T: InputValue<()>,
{
    fn len(&self) -> usize {
        N
    }

    fn get(&self, index: usize) -> Option<ValueRef<'_>> {
        self.as_slice().get(index).map(view_item)
    }
}

impl<T> ListSource for VecDeque<T>
where
    T: InputValue<()>,
{
    fn len(&self) -> usize {
        VecDeque::len(self)
    }

    fn get(&self, index: usize) -> Option<ValueRef<'_>> {
        VecDeque::get(self, index).map(view_item)
    }
}

fn list_at<'a, T, C, I>(
    items: &'a T,
    ctx: &'a C,
    path: &[Segment],
    index_of: impl FnOnce(&'a T, usize) -> Option<&'a I>,
) -> Result<Option<Val<'a>>, BoxError>
where
    T: ListSource,
    C: ?Sized,
    I: InputValue<C> + ?Sized + 'a,
{
    let Some((head, rest)) = path.split_first() else {
        return Ok(Some(Val::Ref(ValueRef::Array(ArrayRef::List(items)))));
    };
    match head {
        Segment::Index(index) => match index_of(items, *index) {
            Some(item) => InputValue::get(item, ctx, rest),
            None => Ok(None),
        },
        Segment::Key { .. } => Ok(None),
    }
}

impl<C, T> InputValue<C> for Vec<T>
where
    C: ?Sized,
    T: InputValue<C> + InputValue<()>,
{
    fn get<'a>(&'a self, ctx: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        list_at(self, ctx, path, |items, index| items.as_slice().get(index))
    }
}

impl<C, T> InputValue<C> for VecDeque<T>
where
    C: ?Sized,
    T: InputValue<C> + InputValue<()>,
{
    fn get<'a>(&'a self, ctx: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        list_at(self, ctx, path, |items, index| VecDeque::get(items, index))
    }
}

impl<C, T, const N: usize> InputValue<C> for [T; N]
where
    C: ?Sized,
    T: InputValue<C> + InputValue<()>,
{
    fn get<'a>(&'a self, ctx: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        list_at(self, ctx, path, |items, index| items.as_slice().get(index))
    }
}

impl<C, T> InputValue<C> for &[T]
where
    C: ?Sized,
    T: InputValue<C> + InputValue<()>,
{
    fn get<'a>(&'a self, ctx: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        // `self` is `&&[T]`: a reference to the caller's slice pointer, which
        // is sized and implements [`ListSource`].
        list_at(self, ctx, path, |items, index| <[T]>::get(items, index))
    }
}

fn object_at<'a, T, C, V>(
    object: &'a T,
    ctx: &'a C,
    path: &[Segment],
    lookup: impl FnOnce(&'a T, &str) -> Option<&'a V>,
) -> Result<Option<Val<'a>>, BoxError>
where
    T: ObjectSource,
    C: ?Sized,
    V: InputValue<C> + ?Sized + 'a,
{
    let Some((head, rest)) = path.split_first() else {
        return Ok(Some(Val::Ref(ValueRef::Object(ObjectRef::Source(object)))));
    };
    match head {
        Segment::Key { key, .. } => match lookup(object, key) {
            Some(value) => InputValue::get(value, ctx, rest),
            None => Ok(None),
        },
        Segment::Index(_) => Ok(None),
    }
}

impl<K, V, S> ObjectSource for HashMap<K, V, S>
where
    K: Eq + Hash + Borrow<str>,
    V: InputValue<()>,
    S: BuildHasher,
{
    fn get(&self, key: &str) -> Option<Val<'_>> {
        self.get(key)
            .and_then(|value| value.get(&(), &[]).ok().flatten())
    }
}

impl<K, V> ObjectSource for BTreeMap<K, V>
where
    K: Ord + Borrow<str>,
    V: InputValue<()>,
{
    fn get(&self, key: &str) -> Option<Val<'_>> {
        self.get(key)
            .and_then(|value| value.get(&(), &[]).ok().flatten())
    }
}

impl<C, K, V, S> InputValue<C> for HashMap<K, V, S>
where
    C: ?Sized,
    K: Eq + Hash + Borrow<str>,
    V: InputValue<C> + InputValue<()>,
    S: BuildHasher,
{
    fn get<'a>(&'a self, ctx: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        object_at(self, ctx, path, |map, key| map.get(key))
    }
}

impl<C, K, V, S> Input<C> for HashMap<K, V, S>
where
    C: ?Sized,
    K: Eq + Hash + Borrow<str>,
    V: InputValue<C> + InputValue<()>,
    S: BuildHasher,
{
    fn get<'a>(&'a self, ctx: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        InputValue::get(self, ctx, path)
    }
}

impl<C, K, V> InputValue<C> for BTreeMap<K, V>
where
    C: ?Sized,
    K: Ord + Borrow<str>,
    V: InputValue<C> + InputValue<()>,
{
    fn get<'a>(&'a self, ctx: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        object_at(self, ctx, path, |map, key| map.get(key))
    }
}

impl<C, K, V> Input<C> for BTreeMap<K, V>
where
    C: ?Sized,
    K: Ord + Borrow<str>,
    V: InputValue<C> + InputValue<()>,
{
    fn get<'a>(&'a self, ctx: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        InputValue::get(self, ctx, path)
    }
}

impl ListSource for serde_json::Value {
    fn len(&self) -> usize {
        match self {
            serde_json::Value::Array(items) => items.len(),
            _ => 0,
        }
    }

    fn get(&self, index: usize) -> Option<ValueRef<'_>> {
        match self {
            serde_json::Value::Array(items) => items.as_slice().get(index).map(json_view),
            _ => None,
        }
    }
}

impl ObjectSource for serde_json::Value {
    fn get(&self, key: &str) -> Option<Val<'_>> {
        match self {
            serde_json::Value::Object(map) => map.get(key).map(|value| Val::Ref(json_view(value))),
            _ => None,
        }
    }
}

fn json_number(n: &serde_json::Number) -> ValueRef<'static> {
    if let Some(n) = n.as_i64() {
        ValueRef::Int(n)
    } else if let Some(n) = n.as_u64() {
        wide_uint(n)
    } else {
        ValueRef::Float(n.as_f64().unwrap_or(f64::NAN))
    }
}

fn json_view(value: &serde_json::Value) -> ValueRef<'_> {
    match value {
        serde_json::Value::Null => ValueRef::Null,
        serde_json::Value::Bool(b) => ValueRef::Bool(*b),
        serde_json::Value::Number(n) => json_number(n),
        serde_json::Value::String(s) => ValueRef::Str(s),
        serde_json::Value::Array(_) => ValueRef::Array(ArrayRef::List(value)),
        serde_json::Value::Object(_) => ValueRef::Object(ObjectRef::Source(value)),
    }
}

fn json_path<'a>(value: &'a serde_json::Value, path: &[Segment]) -> Option<Val<'a>> {
    let Some((head, rest)) = path.split_first() else {
        return Some(Val::Ref(json_view(value)));
    };
    match (value, head) {
        (serde_json::Value::Object(map), Segment::Key { key, .. }) => {
            json_path(map.get(key.as_str())?, rest)
        }
        (serde_json::Value::Array(items), Segment::Index(index)) => {
            json_path(items.as_slice().get(*index)?, rest)
        }
        _ => None,
    }
}

impl<C: ?Sized> InputValue<C> for serde_json::Value {
    fn get<'a>(&'a self, _: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        Ok(json_path(self, path))
    }
}

impl<C: ?Sized> Input<C> for serde_json::Value {
    fn get<'a>(&'a self, ctx: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        InputValue::get(self, ctx, path)
    }
}
