//! Evaluation inputs.

use std::fmt;
use std::sync::{Arc, Mutex, OnceLock};

use crate::ast::Segment;
use crate::error::BoxError;
use crate::input_value::{InputValue, step};
use crate::value::{Map, ObjectRef, Val, Value, ValueRef};

/// Resolves rule paths (such as `request.headers["host"]` or `tags[0]`)
/// against evaluation data.
///
/// Implemented by [`KvInput`], [`FnInput`], and [`NoInput`], by string-keyed
/// maps and `serde_json` values, by `#[derive(Input)]` types, and by
/// references, `Box`es, and `Arc`s of inputs.
pub trait Input {
    /// The value at `path`, which may borrow from the input.
    ///
    /// Return `Ok(None)` when the path is absent: the rule result is then
    /// unknown, with the field listed in
    /// [`EvalResult::missing_fields`](crate::EvalResult::missing_fields).
    /// An `Err` is reported as [`Error::Input`](crate::Error::Input).
    fn get<'a>(&'a self, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError>;
}

impl<T: Input + ?Sized> Input for &T {
    fn get<'a>(&'a self, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        (**self).get(path)
    }
}

impl<T: Input + ?Sized> Input for Box<T> {
    fn get<'a>(&'a self, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        (**self).get(path)
    }
}

impl<T: Input + ?Sized> Input for Arc<T> {
    fn get<'a>(&'a self, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        (**self).get(path)
    }
}

/// An input with no fields: every path is missing.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoInput;

impl Input for NoInput {
    fn get<'a>(&'a self, _: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        Ok(None)
    }
}

/// An input backed by a closure that resolves a path to an owned value.
///
/// Return `Ok(None)` for an absent field. Nested in a [`KvInput`] as a
/// [`KvEntry::Input`], the closure receives the rest of the path below its
/// key.
///
/// ```rust
/// use rulekit::ast::Segment;
/// use rulekit::value::Value;
/// use rulekit::{BoxError, FnInput, Opts};
///
/// let input = FnInput(|path: &[Segment]| {
///     Ok::<_, BoxError>(match path {
///         [Segment::Key { key, .. }] if key == "port" => Some(Value::Int(443)),
///         _ => None,
///     })
/// });
/// let opts = Opts::default();
///
/// let rule = rulekit::parse("port == 443")?;
/// assert!(rule.eval(&input, &opts).pass());
///
/// let rule = rulekit::parse("host == \"example.com\"")?;
/// assert_eq!(rule.eval(&input, &opts).missing_fields().collect::<Vec<_>>(), ["host"]);
/// # Ok::<(), rulekit::ParseError>(())
/// ```
pub struct FnInput<F>(pub F);

impl<F> Input for FnInput<F>
where
    F: Fn(&[Segment]) -> Result<Option<Value>, BoxError>,
{
    fn get<'a>(&'a self, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        Ok((self.0)(path)?.map(Val::Owned))
    }
}

/// A key-value tree for [`KvInput`]: field names to [`KvEntry`]s.
///
/// Build one by hand (it is a `HashMap`, so `Kv::from_iter` and `insert`
/// work) or decode it from JSON with [`decode_json`](crate::decode_json).
pub type Kv = Map<KvEntry>;

/// One entry of a [`Kv`] map.
///
/// `Value` converts into `KvEntry::Value` with `into()`.
#[derive(Clone)]
pub enum KvEntry {
    /// Plain data, including nested plain objects (`Value::Object`).
    Value(Value),
    /// A nested map whose entries may themselves be inputs.
    Object(Kv),
    /// A nested input that resolves the rest of any path through it.
    Input(Arc<dyn Input + Send + Sync>),
    /// A value computed the first time it is read.
    Lazy(Lazy),
}

type LazyFn = dyn Fn() -> Result<KvEntry, BoxError> + Send + Sync;

/// A value computed the first time a rule reads it, then memoized in this
/// entry.
///
/// Resolution is single-flight: concurrent evaluations sharing one
/// [`KvInput`] call the function once. Errors are not memoized, so a later
/// read retries. Once resolved, reads take no lock. Cloning gives a fresh,
/// unresolved entry, so memoization is per input instance.
pub struct Lazy {
    f: Arc<LazyFn>,
    value: OnceLock<Box<KvEntry>>,
    lock: Mutex<()>,
}

impl Lazy {
    /// A lazy entry computed by `f`, which returns a [`Value`] or a
    /// [`KvEntry`] (for example a [`KvEntry::Object`] or [`KvEntry::Input`]
    /// to resolve the rest of the path).
    pub fn new<T, F>(f: F) -> Self
    where
        T: Into<KvEntry>,
        F: Fn() -> Result<T, BoxError> + Send + Sync + 'static,
    {
        Lazy {
            f: Arc::new(move || f().map(Into::into)),
            value: OnceLock::new(),
            lock: Mutex::new(()),
        }
    }

    fn resolve(&self) -> Result<&KvEntry, BoxError> {
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
        let v = Box::new((self.f)()?);
        Ok(&**self.value.get_or_init(|| v))
    }
}

impl Clone for Lazy {
    fn clone(&self) -> Self {
        Lazy {
            f: self.f.clone(),
            value: OnceLock::new(),
            lock: Mutex::new(()),
        }
    }
}

impl InputValue for Lazy {
    fn get<'a>(&'a self, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        let resolved = self.resolve()?;
        let cursor = match resolved {
            KvEntry::Value(v) => Cursor::Val(Val::Ref(v.as_ref())),
            KvEntry::Object(kv) => Cursor::Kv(kv),
            KvEntry::Input(input) => Cursor::Input(input.as_ref()),
            // A lazy that yields another lazy is not resolved again.
            KvEntry::Lazy(_) => Cursor::Val(Val::Ref(ValueRef::Object(ObjectRef::Opaque))),
        };
        walk(cursor, path)
    }
}

impl Input for Lazy {
    fn get<'a>(&'a self, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        InputValue::get(self, path)
    }
}

impl fmt::Debug for Lazy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Lazy")
            .field("resolved", &self.value.get().is_some())
            .finish()
    }
}

impl From<Value> for KvEntry {
    fn from(v: Value) -> Self {
        KvEntry::Value(v)
    }
}

impl From<String> for KvEntry {
    fn from(s: String) -> Self {
        Value::String(s).into()
    }
}

impl From<&str> for KvEntry {
    fn from(s: &str) -> Self {
        Value::String(s.to_owned()).into()
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
/// use rulekit::{BoxError, FnInput, Kv, KvEntry, KvInput, Lazy, Opts};
///
/// // Resolves `request.headers.<name>`.
/// let headers = FnInput(|path: &[Segment]| {
///     Ok::<_, BoxError>(match path {
///         [Segment::Key { key, .. }] if key == "host" => {
///             Some(Value::String("example.com".into()))
///         }
///         _ => None,
///     })
/// });
///
/// let request: Kv = Kv::from_iter([
///     ("method".to_owned(), Value::String("GET".into()).into()),
///     ("headers".to_owned(), KvEntry::Input(Arc::new(headers))),
/// ]);
/// let input = KvInput::new(Kv::from_iter([
///     ("request".to_owned(), KvEntry::Object(request)),
///     (
///         "user".to_owned(),
///         KvEntry::Lazy(Lazy::new(|| Ok(Value::String("alice".into())))),
///     ),
/// ]));
///
/// let rule = rulekit::parse(
///     r#"request.method == "GET" and request.headers.host == "example.com" and user == "alice""#,
/// )?;
/// assert!(rule.eval(&input, &Opts::default()).pass());
/// # Ok::<(), rulekit::ParseError>(())
/// ```
pub struct KvInput {
    kv: Kv,
}

impl KvInput {
    /// An input over `kv`.
    pub fn new(kv: Kv) -> Self {
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

enum Cursor<'a> {
    Kv(&'a Kv),
    Input(&'a (dyn Input + Send + Sync)),
    Val(Val<'a>),
}

impl Input for KvInput {
    fn get<'a>(&'a self, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        if path.is_empty() {
            return Ok(None);
        }
        walk(Cursor::Kv(&self.kv), path)
    }
}

fn walk<'a>(mut cursor: Cursor<'a>, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
    for (i, segment) in path.iter().enumerate() {
        if let Cursor::Input(input) = cursor {
            return input.get(&path[i..]);
        }
        let next = match cursor {
            Cursor::Val(v) => step(v, segment).map(Cursor::Val),
            Cursor::Kv(kv) => match segment {
                Segment::Key { key, .. } => kv.get(key).map(entry).transpose()?,
                Segment::Index(_) => None,
            },
            Cursor::Input(_) => unreachable!("inputs take over the rest of the path"),
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

/// The cursor for a map entry, resolving a lazy entry (Go `resolveLazy`).
/// A lazy that yields another lazy is not resolved again: like a Go func
/// value, it is an opaque value.
fn entry(entry: &KvEntry) -> Result<Cursor<'_>, BoxError> {
    let entry = match entry {
        KvEntry::Lazy(lazy) => lazy.resolve()?,
        other => other,
    };
    Ok(match entry {
        KvEntry::Value(v) => Cursor::Val(Val::Ref(v.as_ref())),
        KvEntry::Object(kv) => Cursor::Kv(kv),
        KvEntry::Input(input) => Cursor::Input(input.as_ref()),
        KvEntry::Lazy(_) => Cursor::Val(Val::Ref(ValueRef::Object(ObjectRef::Opaque))),
    })
}
