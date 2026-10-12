//! Ad-hoc input: [`kv!`](crate::kv) and [`lazy`].

use crate::ast::Segment;
use crate::error::BoxError;
use crate::input::{Input, KvEntry, Lazy};
use crate::input_value::InputValue;
use crate::value::{ObjectRef, Val, ValueRef};

/// One entry of a [`kv!`](crate::kv) map, then the remaining entries.
///
/// Lookup is a scan in declaration order; the first key wins. Building the
/// value does not allocate. Evaluation does not allocate when the stored
/// values can be borrowed.
pub struct KvList<V, R> {
    /// The field name. Always a string literal from [`kv!`](crate::kv).
    pub key: &'static str,
    /// The field value.
    pub value: V,
    /// The remaining entries.
    pub rest: R,
}

/// The end of a [`kv!`](crate::kv) map.
#[derive(Clone, Copy, Debug, Default)]
pub struct KvEnd;

/// A field computed the first time a rule reads it, then memoized on this
/// value.
///
/// Same cell as [`Lazy`]: one resolution per instance, single-flight if
/// several evaluations share it, and a failed resolution is not stored (the
/// next read retries). The closure returns an owned value (`String`, `&str`
/// copied into the memo, [`crate::value::Value`], or [`KvEntry`]).
///
/// ```rust
/// use rulekit::{BoxError, Opts, kv, lazy};
///
/// let input = kv! {
///     "host" => "api.acme.com",
///     "user" => lazy(|| -> Result<String, BoxError> { panic!("user is never read") }),
/// };
/// let rule = rulekit::parse(r#"host == "api.acme.com""#)?;
/// assert!(rule.eval(&input, &Opts::default()).pass());
/// # Ok::<(), rulekit::ParseError>(())
/// ```
pub fn lazy<T, F>(f: F) -> Lazy
where
    T: Into<KvEntry>,
    F: Fn() -> Result<T, BoxError> + Send + Sync + 'static,
{
    Lazy::new(f)
}

impl<V, R> InputValue for KvList<V, R>
where
    V: InputValue,
    R: InputValue,
{
    fn get<'a>(&'a self, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        let Some((head, rest)) = path.split_first() else {
            return Ok(Some(Val::Ref(ValueRef::Object(ObjectRef::Opaque))));
        };
        match head {
            Segment::Key { key, .. } if key == self.key => InputValue::get(&self.value, rest),
            Segment::Key { .. } => InputValue::get(&self.rest, path),
            Segment::Index(_) => Ok(None),
        }
    }
}

impl<V, R> Input for KvList<V, R>
where
    V: InputValue,
    R: Input + InputValue,
{
    fn get<'a>(&'a self, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        InputValue::get(self, path)
    }
}

impl InputValue for KvEnd {
    fn get<'a>(&'a self, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        if path.is_empty() {
            Ok(Some(Val::Ref(ValueRef::Object(ObjectRef::Opaque))))
        } else {
            Ok(None)
        }
    }
}

impl Input for KvEnd {
    fn get<'a>(&'a self, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        InputValue::get(self, path)
    }
}

/// Build an input from field names and values.
///
/// Values are anything that implements [`InputValue`](crate::InputValue):
/// plain numbers and strings, maps, lists, derived structs, or [`lazy`]
/// closures. A nested `{ ... }` is another map. Keys are string literals.
/// The first of two equal keys wins.
///
/// ```rust
/// use std::collections::HashMap;
/// use rulekit::{Opts, kv};
///
/// let headers = HashMap::from([("x-env".to_owned(), "prod".to_owned())]);
/// let input = kv! {
///     "host" => "api.acme.com",
///     "port" => 8443,
///     "headers" => &headers,
///     "user" => { "id" => 42 },
/// };
/// let rule = rulekit::parse(
///     r#"host == "api.acme.com" and port == 8443 and headers["x-env"] == "prod" and user.id == 42"#,
/// )?;
/// assert!(rule.eval(&input, &Opts::default()).pass());
/// # Ok::<(), rulekit::ParseError>(())
/// ```
#[macro_export]
macro_rules! kv {
    ({ $($body:tt)* }) => { $crate::__kv_parse!([] $($body)*) };
    ($($body:tt)*) => { $crate::__kv_parse!([] $($body)*) };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __kv_parse {
    ([$($built:tt)*] $key:literal => { $($nested:tt)* } $(, $($rest:tt)*)?) => {
        $crate::__kv_parse!([$($built)* ($key, $crate::kv! { $($nested)* })] $($($rest)*)?)
    };
    ([$($built:tt)*] $key:literal => $val:expr $(, $($rest:tt)*)?) => {
        $crate::__kv_parse!([$($built)* ($key, $val)] $($($rest)*)?)
    };
    ([$(($key:literal, $val:expr))*]) => {
        $crate::__kv_finish!($(($key, $val))*)
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __kv_finish {
    () => { $crate::KvEnd };
    (($key:literal, $val:expr) $($rest:tt)*) => {
        $crate::KvList {
            key: $key,
            value: $val,
            rest: $crate::__kv_finish!($($rest)*),
        }
    };
}
