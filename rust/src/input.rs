//! Evaluation inputs (port of `input.go` and the path walk in `values.go`).

use std::fmt;
use std::sync::{Arc, Mutex, OnceLock};

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
    /// A value computed on first use (Go `LazyValue`/`LazyContextValue`).
    Lazy(Lazy<C>),
}

type LazyFn<C> = dyn Fn(&C) -> Result<KvEntry<C>, BoxError> + Send + Sync;

/// A value computed from the evaluation context the first time a rule reads
/// it, then memoized in this entry.
///
/// Resolution is single-flight: concurrent evaluations sharing one
/// [`KvInput`] call the function once. Errors are not memoized, so a later
/// read retries. Once resolved, reads take no lock. Cloning gives a fresh,
/// unresolved entry, so memoization is per input instance.
pub struct Lazy<C: ?Sized = ()> {
    f: Arc<LazyFn<C>>,
    value: OnceLock<Box<KvEntry<C>>>,
    lock: Mutex<()>,
}

impl<C: ?Sized> Lazy<C> {
    pub fn new<T, F>(f: F) -> Self
    where
        T: Into<KvEntry<C>>,
        F: Fn(&C) -> Result<T, BoxError> + Send + Sync + 'static,
    {
        Lazy {
            f: Arc::new(move |ctx: &C| f(ctx).map(Into::into)),
            value: OnceLock::new(),
            lock: Mutex::new(()),
        }
    }

    fn resolve(&self, ctx: &C) -> Result<&KvEntry<C>, BoxError> {
        if let Some(v) = self.value.get() {
            return Ok(&**v);
        }
        // Held across the call: one resolution per entry, while other
        // entries resolve independently.
        let _guard = self
            .lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(v) = self.value.get() {
            return Ok(&**v);
        }
        let v = Box::new((self.f)(ctx)?);
        Ok(&**self.value.get_or_init(|| v))
    }
}

impl<C: ?Sized> Clone for Lazy<C> {
    fn clone(&self) -> Self {
        Lazy {
            f: self.f.clone(),
            value: OnceLock::new(),
            lock: Mutex::new(()),
        }
    }
}

impl<C: ?Sized> fmt::Debug for Lazy<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Lazy")
            .field("resolved", &self.value.get().is_some())
            .finish()
    }
}

impl<C: ?Sized> Clone for KvEntry<C> {
    fn clone(&self) -> Self {
        match self {
            KvEntry::Value(v) => KvEntry::Value(v.clone()),
            KvEntry::Object(kv) => KvEntry::Object(kv.clone()),
            KvEntry::Input(input) => KvEntry::Input(input.clone()),
            KvEntry::Lazy(lazy) => KvEntry::Lazy(lazy.clone()),
        }
    }
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
                Segment::Key { key, .. } => key_value(cursor, key, ctx)?,
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

/// The cursor for a map entry, resolving a lazy entry (Go `resolveLazy`).
/// A lazy that yields another lazy is not resolved again: like a Go func
/// value, it is an opaque value.
fn entry<'a, C: ?Sized>(entry: &'a KvEntry<C>, ctx: &'a C) -> Result<Cursor<'a, C>, BoxError> {
    let entry = match entry {
        KvEntry::Lazy(lazy) => lazy.resolve(ctx)?,
        other => other,
    };
    Ok(match entry {
        KvEntry::Value(v) => Cursor::Val(Val::Ref(v.as_ref())),
        KvEntry::Object(kv) => Cursor::Kv(kv),
        KvEntry::Input(input) => Cursor::Input(input.as_ref()),
        KvEntry::Lazy(_) => Cursor::Val(Val::Ref(ValueRef::Object(ObjectRef::Opaque))),
    })
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
fn key_value<'a, C: ?Sized>(
    cursor: Cursor<'a, C>,
    key: &str,
    ctx: &'a C,
) -> Result<Option<Cursor<'a, C>>, BoxError> {
    Ok(match cursor {
        Cursor::Kv(kv) => match kv.get(key) {
            Some(e) => Some(entry(e, ctx)?),
            None => None,
        },
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
    })
}
