//! Evaluation inputs.

use std::fmt;
use std::sync::{Arc, Mutex, OnceLock};

use crate::ast::Segment;
use crate::error::BoxError;
use crate::value::{Map, ObjectRef, Val, Value, ValueRef, value_field};

/// Resolves rule paths (such as `request.headers["host"]` or `tags[0]`)
/// against evaluation data.
///
/// Implemented by [`KvInput`], [`FnInput`], and [`NoInput`], and by
/// references, `Box`es, and `Arc`s of inputs. `C` is the caller's
/// evaluation context type.
pub trait Input<C: ?Sized = ()> {
    /// The value at `path`, which may borrow from the input or `ctx`.
    ///
    /// Return `Ok(None)` when the path is absent: the rule result is then
    /// unknown, with the field listed in
    /// [`EvalResult::missing_fields`](crate::EvalResult::missing_fields).
    /// An `Err` is reported as [`Error::Input`](crate::Error::Input).
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

/// An input with no fields: every path is missing.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoInput;

impl<C: ?Sized> Input<C> for NoInput {
    fn get<'a>(&'a self, _: &'a C, _: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        Ok(None)
    }
}

/// An input backed by a closure that resolves a path to an owned value.
///
/// The closure receives the evaluation context and the path; return
/// `Ok(None)` for an absent field. Nested in a [`KvInput`] as a
/// [`KvEntry::Input`], it receives the rest of the path below its key.
///
/// ```rust
/// use rulekit::ast::Segment;
/// use rulekit::value::Value;
/// use rulekit::{BoxError, FnInput, Opts};
///
/// let input = FnInput(|_: &(), path: &[Segment]| {
///     Ok::<_, BoxError>(match path {
///         [Segment::Key { key, .. }] if key == "port" => Some(Value::Int(443)),
///         _ => None,
///     })
/// });
///
/// let rule = rulekit::parse("port == 443")?;
/// assert!(rule.eval(&(), &input, Opts::default()).pass());
///
/// let rule = rulekit::parse("host == \"example.com\"")?;
/// assert_eq!(rule.eval(&(), &input, Opts::default()).missing_fields(), ["host"]);
/// # Ok::<(), rulekit::ParseError>(())
/// ```
pub struct FnInput<F>(pub F);

impl<C: ?Sized, F> Input<C> for FnInput<F>
where
    F: Fn(&C, &[Segment]) -> Result<Option<Value>, BoxError>,
{
    fn get<'a>(&'a self, ctx: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        Ok((self.0)(ctx, path)?.map(Val::Owned))
    }
}

/// A key-value tree for [`KvInput`]: field names to [`KvEntry`]s.
///
/// Build one by hand (it is a `HashMap`, so `Kv::from_iter` and `insert`
/// work) or decode it from JSON with [`decode_json`](crate::decode_json).
pub type Kv<C = ()> = Map<KvEntry<C>>;

/// One entry of a [`Kv`] map.
///
/// `Value` converts into `KvEntry::Value` with `into()`.
pub enum KvEntry<C: ?Sized = ()> {
    /// Plain data, including nested plain objects (`Value::Object`).
    Value(Value),
    /// A nested map whose entries may themselves be inputs.
    Object(Kv<C>),
    /// A nested input that resolves the rest of any path through it.
    Input(Arc<dyn Input<C> + Send + Sync>),
    /// A value computed from the context on first use.
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
    /// A lazy entry computed by `f`, which returns a [`Value`] or a
    /// [`KvEntry`] (for example a [`KvEntry::Object`] or [`KvEntry::Input`]
    /// to resolve the rest of the path).
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

/// An input over a [`Kv`] tree.
///
/// Each path segment selects a map entry, an array element, or a built-in
/// field of a typed value (such as `url.host`). Dotted paths traverse nested
/// maps; they never fall back to flat keys, so `a.b` does not read a key
/// named `"a.b"` (write `["a.b"]` for that). A [`KvEntry::Input`] resolves
/// the rest of any path through it, and a [`KvEntry::Lazy`] is computed when
/// first read.
///
/// ```rust
/// use std::sync::Arc;
/// use rulekit::ast::Segment;
/// use rulekit::value::Value;
/// use rulekit::{BoxError, Env, FnInput, Kv, KvEntry, KvInput, Lazy, Opts};
///
/// struct Ctx {
///     user: String,
/// }
///
/// // Resolves `request.headers.<name>`.
/// let headers = FnInput(|_: &Ctx, path: &[Segment]| {
///     Ok::<_, BoxError>(match path {
///         [Segment::Key { key, .. }] if key == "host" => {
///             Some(Value::String("example.com".into()))
///         }
///         _ => None,
///     })
/// });
///
/// let request: Kv<Ctx> = Kv::from_iter([
///     ("method".to_owned(), Value::String("GET".into()).into()),
///     ("headers".to_owned(), KvEntry::Input(Arc::new(headers))),
/// ]);
/// let input = KvInput::new(Kv::from_iter([
///     ("request".to_owned(), KvEntry::Object(request)),
///     (
///         "user".to_owned(),
///         KvEntry::Lazy(Lazy::new(|ctx: &Ctx| Ok(Value::String(ctx.user.clone())))),
///     ),
/// ]));
///
/// let rule = rulekit::parse(
///     r#"request.method == "GET" and request.headers.host == "example.com" and user == "alice""#,
/// )?;
/// let env = Env::new();
/// let ctx = Ctx { user: "alice".into() };
/// assert!(rule.eval(&ctx, &input, Opts::new(&env)).pass());
/// # Ok::<(), rulekit::ParseError>(())
/// ```
pub struct KvInput<C: ?Sized = ()> {
    kv: Kv<C>,
}

impl<C: ?Sized> KvInput<C> {
    /// An input over `kv`.
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
