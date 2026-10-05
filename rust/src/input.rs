//! Evaluation inputs (port of `input.go` and the path walk in `values.go`).

use std::sync::Arc;

use crate::ast::Segment;
use crate::error::BoxError;
use crate::value::{Map, ObjectRef, Val, Value, ValueRef, value_field};

/// Resolves rule paths against an evaluation input. `ctx` is the caller's
/// evaluation context.
///
/// Return `Ok(None)` when the path is absent: the rule result is then
/// unknown with that field missing.
pub trait Input<C: ?Sized = ()> {
    fn get<'a>(&'a self, ctx: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError>;
}

impl<C: ?Sized, T: Input<C> + ?Sized> Input<C> for &T {
    fn get<'a>(&'a self, ctx: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        (**self).get(ctx, path)
    }
}

impl<C: ?Sized, T: Input<C> + ?Sized> Input<C> for Box<T> {
    fn get<'a>(&'a self, ctx: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        (**self).get(ctx, path)
    }
}

impl<C: ?Sized, T: Input<C> + ?Sized> Input<C> for Arc<T> {
    fn get<'a>(&'a self, ctx: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        (**self).get(ctx, path)
    }
}

/// An input with no fields: every path is missing (Go's nil input).
#[derive(Clone, Copy, Debug, Default)]
pub struct NoInput;

impl<C: ?Sized> Input<C> for NoInput {
    fn get<'a>(&'a self, _: &'a C, _: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        Ok(None)
    }
}

/// An input backed by a function returning owned values (Go `FromFunc` /
/// `FromContextFunc`).
pub struct FnInput<F>(pub F);

impl<C: ?Sized, F> Input<C> for FnInput<F>
where
    F: Fn(&C, &[Segment]) -> Result<Option<Value>, BoxError>,
{
    fn get<'a>(&'a self, ctx: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        Ok((self.0)(ctx, path)?.map(Val::Owned))
    }
}

/// A key-value tree for [`KvInput`].
pub type Kv<C = ()> = Map<KvEntry<C>>;

/// One entry of a [`Kv`] map.
pub enum KvEntry<C: ?Sized = ()> {
    /// Plain data, including nested plain objects (`Value::Object`).
    Value(Value),
    /// A nested map whose entries may themselves be inputs.
    Object(Kv<C>),
    /// A nested input that resolves the rest of any path through it.
    Input(Arc<dyn Input<C> + Send + Sync>),
}

impl<C: ?Sized> From<Value> for KvEntry<C> {
    fn from(v: Value) -> Self {
        KvEntry::Value(v)
    }
}

/// An input over a [`Kv`] tree (Go `FromKV`). Dotted paths traverse nested
/// maps; they never fall back to flat keys.
pub struct KvInput<C: ?Sized = ()> {
    kv: Kv<C>,
}

impl<C: ?Sized> KvInput<C> {
    pub fn new(kv: Kv<C>) -> Self {
        KvInput { kv }
    }

    /// Build from plain values.
    pub fn from_values(values: Map<Value>) -> Self {
        KvInput {
            kv: values
                .into_iter()
                .map(|(k, v)| (k, KvEntry::Value(v)))
                .collect(),
        }
    }
}

enum Cursor<'a, C: ?Sized> {
    Kv(&'a Kv<C>),
    Input(&'a (dyn Input<C> + Send + Sync)),
    Val(Val<'a>),
}

impl<C: ?Sized> Input<C> for KvInput<C> {
    fn get<'a>(&'a self, ctx: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        if path.is_empty() {
            return Ok(None);
        }
        let mut cursor = Cursor::Kv(&self.kv);
        for (i, segment) in path.iter().enumerate() {
            if let Cursor::Input(input) = cursor {
                return input.get(ctx, &path[i..]);
            }
            let next = match segment {
                Segment::Index(index) => index_value(cursor, *index),
                Segment::Key { key, .. } => key_value(cursor, key),
            };
            match next {
                Some(next) => cursor = next,
                None => return Ok(None),
            }
        }
        Ok(Some(match cursor {
            Cursor::Kv(_) | Cursor::Input(_) => Val::Ref(ValueRef::Object(ObjectRef::Opaque)),
            Cursor::Val(v) => v,
        }))
    }
}

fn entry<C: ?Sized>(entry: &KvEntry<C>) -> Cursor<'_, C> {
    match entry {
        KvEntry::Value(v) => Cursor::Val(Val::Ref(v.as_ref())),
        KvEntry::Object(kv) => Cursor::Kv(kv),
        KvEntry::Input(input) => Cursor::Input(input.as_ref()),
    }
}

/// Go `indexAny`: only lists can be indexed.
fn index_value<C: ?Sized>(cursor: Cursor<'_, C>, index: usize) -> Option<Cursor<'_, C>> {
    let Cursor::Val(v) = cursor else { return None };
    let value = match v {
        Val::Ref(ValueRef::Array(a)) => Val::Ref(a.get(index)?),
        Val::Owned(Value::Array(mut items)) if index < items.len() => {
            Val::Owned(items.swap_remove(index))
        }
        _ => return None,
    };
    Some(Cursor::Val(value))
}

/// A map key, or a built-in field of a typed value.
fn key_value<'a, C: ?Sized>(cursor: Cursor<'a, C>, key: &str) -> Option<Cursor<'a, C>> {
    match cursor {
        Cursor::Kv(kv) => kv.get(key).map(entry),
        Cursor::Input(_) => unreachable!("inputs take over the rest of the path"),
        Cursor::Val(Val::Ref(ValueRef::Object(ObjectRef::Map(map)))) => {
            map.get(key).map(|v| Cursor::Val(Val::Ref(v.as_ref())))
        }
        Cursor::Val(Val::Ref(ValueRef::Object(ObjectRef::Opaque))) => None,
        Cursor::Val(Val::Ref(v)) => value_field(v, key).map(Cursor::Val),
        Cursor::Val(Val::Owned(Value::Object(mut map))) => {
            map.remove(key).map(|v| Cursor::Val(Val::Owned(v)))
        }
        Cursor::Val(Val::Owned(v)) => {
            value_field(v.as_ref(), key).map(|f| Cursor::Val(Val::Owned(f.into_owned())))
        }
    }
}
