//! Ad-hoc input: [`kv!`](crate::kv) and [`lazy`].

use crate::ast::Segment;
use crate::error::BoxError;
use crate::input::Input;
use crate::input_value::{InputValue, project};
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

type LazyFn<'f, C> = dyn for<'a> Fn(&'a C) -> Result<Val<'a>, BoxError> + Send + Sync + 'f;

/// A value computed from the evaluation context when a rule reads it.
///
/// Not memoized: each read calls the closure. The closure may return data
/// borrowed from the context. Constructing this boxes the closure; evaluation
/// does not allocate if the closure does not.
pub struct LazyVal<'f, C: ?Sized> {
    f: Box<LazyFn<'f, C>>,
}

impl<C: ?Sized> LazyVal<'_, C> {
    fn call<'a>(&self, ctx: &'a C) -> Result<Val<'a>, BoxError> {
        (self.f)(ctx)
    }
}

/// A field computed only if a rule reads it.
///
/// ```rust
/// use rulekit::{Opts, lazy, kv};
///
/// struct Ctx {
///     user: String,
/// }
///
/// let input = kv! {
///     "host" => "api.acme.com",
///     "user" => lazy(|ctx: &Ctx| Ok(ctx.user.as_str().into())),
/// };
/// let rule = rulekit::parse(r#"host == "api.acme.com""#)?;
/// let env: rulekit::Env<Ctx> = rulekit::Env::new();
/// // `user` is not read, so the closure is not called.
/// assert!(rule.eval(&Ctx { user: "ada".into() }, &input, Opts::new(&env)).pass());
/// # Ok::<(), rulekit::ParseError>(())
/// ```
pub fn lazy<'f, C, F>(f: F) -> LazyVal<'f, C>
where
    C: ?Sized,
    F: for<'a> Fn(&'a C) -> Result<Val<'a>, BoxError> + Send + Sync + 'f,
{
    LazyVal { f: Box::new(f) }
}

impl<C, V, R> InputValue<C> for KvList<V, R>
where
    C: ?Sized,
    V: InputValue<C>,
    R: InputValue<C>,
{
    fn get<'a>(&'a self, ctx: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        let Some((head, rest)) = path.split_first() else {
            return Ok(Some(Val::Ref(ValueRef::Object(ObjectRef::Opaque))));
        };
        match head {
            Segment::Key { key, .. } if key == self.key => InputValue::get(&self.value, ctx, rest),
            Segment::Key { .. } => InputValue::get(&self.rest, ctx, path),
            Segment::Index(_) => Ok(None),
        }
    }
}

impl<C, V, R> Input<C> for KvList<V, R>
where
    C: ?Sized,
    V: InputValue<C>,
    R: Input<C> + InputValue<C>,
{
    fn get<'a>(&'a self, ctx: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        InputValue::get(self, ctx, path)
    }
}

impl<C: ?Sized> InputValue<C> for KvEnd {
    fn get<'a>(&'a self, _: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        if path.is_empty() {
            Ok(Some(Val::Ref(ValueRef::Object(ObjectRef::Opaque))))
        } else {
            Ok(None)
        }
    }
}

impl<C: ?Sized> Input<C> for KvEnd {
    fn get<'a>(&'a self, ctx: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        InputValue::get(self, ctx, path)
    }
}

impl<C: ?Sized> InputValue<C> for LazyVal<'_, C> {
    fn get<'a>(&'a self, ctx: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        Ok(project(self.call(ctx)?, path))
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
/// assert!(rule.eval(&(), &input, Opts::default()).pass());
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
