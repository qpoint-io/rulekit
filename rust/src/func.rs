//! Typed custom functions.
//!
//! A function's arguments are a struct deriving [`Args`](crate::Args) (one
//! field per positional argument) and its return type is any [`Returns`]
//! type. Both are named in the [`FuncSchema`], so the implementation closure
//! needs no type annotations and may borrow from the arguments and from the
//! evaluation context.

use std::borrow::Cow;
use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;

use crate::error::{BoxError, Error};
use crate::value::{Cidr, Ip, Mac, TextForm, Url, Val, Value, ValueRef};

/// One positional parameter of a function: its name, the type name its
/// argument must have (`"any"` for any value), and whether it is the rest
/// parameter taking all remaining arguments.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Param {
    name: &'static str,
    ty: &'static str,
    rest: bool,
}

impl Param {
    /// A parameter named `name` of type `ty` (a typed-JSON type name).
    pub const fn new(name: &'static str, ty: &'static str) -> Self {
        Param {
            name,
            ty,
            rest: false,
        }
    }

    /// The rest parameter named `name`: any number of remaining arguments.
    pub const fn rest(name: &'static str) -> Self {
        Param {
            name,
            ty: "any",
            rest: true,
        }
    }

    /// The parameter name.
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// The type name its argument must have (`"any"` accepts every value).
    pub fn ty(&self) -> &'static str {
        self.ty
    }

    /// Whether this is the rest parameter.
    pub fn is_rest(&self) -> bool {
        self.rest
    }
}

/// The arguments of a function: one positional argument per field.
///
/// Derive it with `#[derive(rulekit::Args)]` (the default `derive`
/// feature) on a struct with named fields, optionally with one lifetime for
/// borrowed arguments:
///
/// ```rust
/// #[derive(rulekit::Args)]
/// struct ClampArgs {
///     n: i64,
///     #[rulekit(rename = "max")]
///     limit: i64,
/// }
///
/// #[derive(rulekit::Args)]
/// struct JoinArgs<'a> {
///     separator: &'a str,
///     parts: rulekit::Rest<'a>,
/// }
/// ```
///
/// Each field type must implement [`FromArg`]; [`ValueRef`] accepts any
/// value. An optional last field of type [`Rest`] takes the remaining
/// arguments. The implementing type is the struct with `'static`; [`Of`](
/// Self::Of) gives it for the borrow of one call.
pub trait Args: 'static {
    /// The arguments of one call, borrowing for `'a`.
    type Of<'a>;
    /// The parameters, in order.
    const PARAMS: &'static [Param];
    /// Convert the evaluated arguments. The caller has checked the count.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArg`] if an argument has the wrong type.
    fn parse<'a>(vals: &'a [Val<'a>]) -> Result<Self::Of<'a>, Error>;
}

/// The remaining arguments of a variadic function: the last field of an
/// [`Args`] struct.
#[derive(Clone, Copy, Debug)]
pub struct Rest<'a> {
    vals: &'a [Val<'a>],
}

impl<'a> Rest<'a> {
    #[doc(hidden)]
    pub fn __new(vals: &'a [Val<'a>]) -> Self {
        Rest { vals }
    }

    /// The number of remaining arguments.
    pub fn len(&self) -> usize {
        self.vals.len()
    }

    /// Whether there are no remaining arguments.
    pub fn is_empty(&self) -> bool {
        self.vals.is_empty()
    }

    /// The remaining argument at `index`.
    pub fn get(&self, index: usize) -> Option<ValueRef<'a>> {
        self.vals.get(index).map(Val::as_ref)
    }

    /// The remaining arguments.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = ValueRef<'a>> + 'a {
        self.vals.iter().map(Val::as_ref)
    }
}

/// Types an argument can be read as. Conversions are exact: an `int64`
/// argument is not a `u64`, and a URL is not a `&str`.
///
/// Implemented for `bool`, `i64`, `u64`, `f64`, `&str`, `&[u8]`, [`Ip`],
/// [`Cidr`], [`Mac`], `&`[`Url`], `&regex::Regex`, [`TextForm`] (a string or
/// any value with a text form), and [`ValueRef`] (any value).
pub trait FromArg<'a>: Sized {
    /// The type name of accepted arguments (`"any"` for every value).
    const TYPE: &'static str;
    /// Convert `value`, or `None` if it has another type.
    fn from_arg(value: ValueRef<'a>) -> Option<Self>;
}

macro_rules! from_arg {
    ($ty:ty, $name:literal, $pat:pat => $out:expr) => {
        impl<'a> FromArg<'a> for $ty {
            const TYPE: &'static str = $name;
            fn from_arg(value: ValueRef<'a>) -> Option<Self> {
                match value {
                    $pat => Some($out),
                    _ => None,
                }
            }
        }
    };
}

from_arg!(bool, "bool", ValueRef::Bool(v) => v);
from_arg!(i64, "int64", ValueRef::Int(v) => v);
from_arg!(u64, "uint64", ValueRef::Uint(v) => v);
from_arg!(f64, "float64", ValueRef::Float(v) => v);
from_arg!(&'a str, "string", ValueRef::Str(v) => v);
from_arg!(&'a [u8], "bytes", ValueRef::Bytes(v) => v);
from_arg!(Ip, "ip", ValueRef::Ip(v) => v);
from_arg!(Cidr, "cidr", ValueRef::Cidr(v) => v);
from_arg!(Mac, "mac", ValueRef::Mac(v) => v);
from_arg!(&'a Url, "url", ValueRef::Url(v) => v);
from_arg!(&'a regex::Regex, "regex", ValueRef::Regex(v) => v);

/// Strings and values with a text form (IPs, CIDRs, MACs, URLs), read as
/// their text.
impl<'a> FromArg<'a> for TextForm<'a> {
    const TYPE: &'static str = "string";
    fn from_arg(value: ValueRef<'a>) -> Option<Self> {
        value.text()
    }
}

impl<'a> FromArg<'a> for ValueRef<'a> {
    const TYPE: &'static str = "any";
    fn from_arg(value: ValueRef<'a>) -> Option<Self> {
        Some(value)
    }
}

/// Return types of functions. The implementing type is the `'static` form;
/// [`Of`](Self::Of) is the type returned by one call, which may borrow from
/// the evaluation context (for example `&'a str` for `&'static str`).
///
/// Implemented for `bool`, `i64`, `u64`, `f64`, `&str`, `String`, `&[u8]`,
/// `Vec<u8>`, [`Ip`], [`Cidr`], [`Mac`], [`Url`], and, for values whose type
/// is only known at run time, [`ValueRef`], [`Val`], and [`Value`].
pub trait Returns: 'static {
    /// The value returned by one call.
    type Of<'a>;
    /// The type name of returned values (`"any"` when only known at run time).
    const TYPE: &'static str;
    /// Convert a returned value.
    fn into_val(value: Self::Of<'_>) -> Val<'_>;
}

macro_rules! returns {
    ($ty:ty, $of:ty, $name:literal, |$v:ident| $out:expr) => {
        impl Returns for $ty {
            type Of<'a> = $of;
            const TYPE: &'static str = $name;
            fn into_val($v: Self::Of<'_>) -> Val<'_> {
                $out
            }
        }
    };
}

returns!(bool, bool, "bool", |v| Val::Ref(ValueRef::Bool(v)));
returns!(i64, i64, "int64", |v| Val::Ref(ValueRef::Int(v)));
returns!(u64, u64, "uint64", |v| Val::Ref(ValueRef::Uint(v)));
returns!(f64, f64, "float64", |v| Val::Ref(ValueRef::Float(v)));
returns!(&'static str, &'a str, "string", |v| Val::Ref(
    ValueRef::Str(v)
));
returns!(String, String, "string", |v| Val::Owned(Value::String(v)));
returns!(&'static [u8], &'a [u8], "bytes", |v| Val::Ref(
    ValueRef::Bytes(v)
));
returns!(Vec<u8>, Vec<u8>, "bytes", |v| Val::Owned(Value::Bytes(v)));
returns!(Ip, Ip, "ip", |v| Val::Ref(ValueRef::Ip(v)));
returns!(Cidr, Cidr, "cidr", |v| Val::Ref(ValueRef::Cidr(v)));
returns!(Mac, Mac, "mac", |v| Val::Ref(ValueRef::Mac(v)));
returns!(Url, Url, "url", |v| Val::Owned(Value::Url(Box::new(v))));
returns!(ValueRef<'static>, ValueRef<'a>, "any", |v| Val::Ref(v));
returns!(Val<'static>, Val<'a>, "any", |v| v);
returns!(Value, Value, "any", |v| Val::Owned(v));

/// A function's name, documentation, and (as type parameters) its argument
/// struct `A` and return type `R`:
///
/// ```rust
/// # #[derive(rulekit::Args)] struct HostArgs<'a> { host: &'a str }
/// use rulekit::FuncSchema;
/// let schema = FuncSchema::<HostArgs, bool>::new("is_internal", "Whether host is internal.");
/// ```
///
/// The name and doc are `Cow<'static, str>`: string literals cost nothing,
/// and names built at run time (for example from configuration) are
/// accepted as `String`s.
pub struct FuncSchema<A, R> {
    name: Cow<'static, str>,
    doc: Cow<'static, str>,
    types: PhantomData<fn() -> (A, R)>,
}

impl<A: Args, R: Returns> FuncSchema<A, R> {
    /// A schema for the function `name`.
    pub fn new(name: impl Into<Cow<'static, str>>, doc: impl Into<Cow<'static, str>>) -> Self {
        FuncSchema {
            name: name.into(),
            doc: doc.into(),
            types: PhantomData,
        }
    }
}

/// Why a custom [`Function`] produced no value.
///
/// `?` converts any error type into [`FnError::Error`].
#[derive(Debug)]
pub enum FnError {
    /// The function needs fields the input lacks: the rule result is unknown
    /// with these fields missing (like a missing field in the rule itself).
    Missing(Vec<String>),
    /// The function failed: the rule result is an [`Error::Function`].
    Error(BoxError),
}

impl FnError {
    /// A [`FnError::Missing`] for the given field names.
    pub fn missing(fields: impl IntoIterator<Item = impl Into<String>>) -> Self {
        FnError::Missing(fields.into_iter().map(Into::into).collect())
    }

    /// A [`FnError::Error`] with a message.
    pub fn msg(message: impl Into<String>) -> Self {
        FnError::Error(message.into().into())
    }
}

impl<E: std::error::Error + Send + Sync + 'static> From<E> for FnError {
    fn from(error: E) -> Self {
        FnError::Error(Box::new(error))
    }
}

/// How a call failed, for the evaluator.
pub(crate) enum CallFailure {
    Error(Error),
    Missing(Vec<String>),
    /// The implementation failed; becomes [`Error::Function`] with the
    /// function's name.
    Failed(BoxError),
}

impl CallFailure {
    /// Name a failed implementation's error after its function.
    pub(crate) fn named(self, name: &str) -> Self {
        match self {
            CallFailure::Failed(source) => CallFailure::Error(Error::Function {
                name: name.to_owned(),
                source,
            }),
            other => other,
        }
    }
}

type Callable<C> =
    dyn for<'a, 's> Fn(&'a C, &'s [Val<'s>]) -> Result<Val<'a>, CallFailure> + Send + Sync;

/// A custom function callable from rules as `name(arg, ...)`.
///
/// Arguments are evaluated first; if one is missing, the call is not made and
/// the result is unknown. The argument count must match the parameters (an
/// [`Error::ArgCount`]); each argument must have its parameter's type (an
/// [`Error::InvalidArg`]). The implementation receives the evaluation context
/// and the arguments. It returns a value, missing fields
/// ([`FnError::Missing`]: the result is unknown), or an error
/// ([`FnError::Error`], reported as [`Error::Function`]).
///
/// Functions only see their arguments and the context, not the rule's input.
/// Register them with [`EnvBuilder::function`](crate::EnvBuilder::function).
///
/// ```rust
/// use rulekit::{Env, FuncSchema, Function, NoInput, Opts};
///
/// struct Ctx {
///     tenant: String,
/// }
///
/// #[derive(rulekit::Args)]
/// struct ClampArgs {
///     n: i64,
///     max: i64,
/// }
///
/// #[derive(rulekit::Args)]
/// struct NoArgs {}
///
/// let clamp = Function::new(
///     FuncSchema::<ClampArgs, i64>::new("clamp", "The smaller of n and max."),
///     |_: &Ctx, a| Ok(a.n.min(a.max)),
/// );
/// // The result borrows from the context.
/// let tenant = Function::new(FuncSchema::<NoArgs, &str>::new("tenant", ""), |ctx: &Ctx, _| {
///     Ok(ctx.tenant.as_str())
/// });
///
/// let env = Env::builder().function(clamp).function(tenant).build()?;
/// let rule = rulekit::parse(r#"clamp(150, 100) == 100 and tenant() == "acme""#)?;
/// let ctx = Ctx { tenant: "acme".into() };
/// assert!(rule.eval(&ctx, &NoInput, Opts::new(&env)).pass());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub struct Function<C: ?Sized = ()> {
    name: Cow<'static, str>,
    doc: Cow<'static, str>,
    params: &'static [Param],
    returns: &'static str,
    call: Arc<Callable<C>>,
}

impl<C: ?Sized> Clone for Function<C> {
    fn clone(&self) -> Self {
        Function {
            name: self.name.clone(),
            doc: self.doc.clone(),
            params: self.params,
            returns: self.returns,
            call: self.call.clone(),
        }
    }
}

impl<C: ?Sized> fmt::Debug for Function<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Function")
            .field("name", &self.name)
            .field("params", &self.params)
            .field("returns", &self.returns)
            .finish()
    }
}

impl<C: ?Sized + 'static> Function<C> {
    /// A function with the schema's name, parameters (from `A`), and return
    /// type (from `R`), implemented by `f`.
    pub fn new<A, R, F>(schema: FuncSchema<A, R>, f: F) -> Self
    where
        A: Args,
        R: Returns,
        F: for<'a, 's> Fn(&'a C, A::Of<'s>) -> Result<R::Of<'a>, FnError> + Send + Sync + 'static,
    {
        Function {
            name: schema.name,
            doc: schema.doc,
            params: A::PARAMS,
            returns: R::TYPE,
            call: Arc::new(move |ctx, vals| invoke::<C, A, R, F>(&f, ctx, vals)),
        }
    }
}

impl<C: ?Sized> Function<C> {
    /// The function name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The documentation.
    pub fn doc(&self) -> &str {
        &self.doc
    }

    /// The parameters, in order.
    pub fn params(&self) -> &'static [Param] {
        self.params
    }

    /// The return type name (`"any"` when only known at run time).
    pub fn returns(&self) -> &'static str {
        self.returns
    }

    /// Check the argument count and run the function.
    pub(crate) fn call<'a>(&self, ctx: &'a C, vals: &[Val<'_>]) -> Result<Val<'a>, CallFailure> {
        (self.call)(ctx, vals).map_err(|failure| failure.named(&self.name))
    }

    /// Check an argument count against the parameters (Go: before the
    /// arguments are evaluated).
    pub(crate) fn check_arity(&self, got: usize) -> Result<(), Error> {
        check_arity(&self.name, self.params, got)
    }
}

pub(crate) fn check_arity(name: &str, params: &[Param], got: usize) -> Result<(), Error> {
    let variadic = params.last().is_some_and(|p| p.rest);
    let fixed = params.len() - usize::from(variadic);
    if got == fixed || (variadic && got > fixed) {
        return Ok(());
    }
    Err(Error::ArgCount {
        function: name.to_owned(),
        expected: fixed,
        got,
        variadic,
    })
}

/// Convert the arguments, call `f`, and convert its result.
pub(crate) fn invoke<'a, C, A, R, F>(
    f: &F,
    ctx: &'a C,
    vals: &[Val<'_>],
) -> Result<Val<'a>, CallFailure>
where
    C: ?Sized,
    A: Args,
    R: Returns,
    F: for<'b, 's> Fn(&'b C, A::Of<'s>) -> Result<R::Of<'b>, FnError>,
{
    let args = A::parse(vals).map_err(CallFailure::Error)?;
    match f(ctx, args) {
        Ok(value) => Ok(R::into_val(value)),
        Err(FnError::Missing(fields)) => Err(CallFailure::Missing(fields)),
        Err(FnError::Error(source)) => Err(CallFailure::Failed(source)),
    }
}

/// Support code for `#[derive(Args)]`. Not public API.
#[doc(hidden)]
pub mod __private {
    use super::FromArg;
    use crate::error::Error;
    use crate::value::Val;

    /// The argument at `index` as `T`.
    pub fn arg<'a, T: FromArg<'a>>(
        vals: &'a [Val<'a>],
        index: usize,
        name: &'static str,
    ) -> Result<T, Error> {
        let value = vals[index].as_ref();
        T::from_arg(value).ok_or_else(|| Error::InvalidArg {
            name: name.to_owned(),
            expected: T::TYPE.to_owned(),
            got: value.type_name().to_owned(),
        })
    }
}
