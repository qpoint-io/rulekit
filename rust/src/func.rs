//! Typed custom functions.
//!
//! A function's arguments are a struct deriving [`Args`](crate::Args) (one
//! field per positional argument) and its return type is any [`Returns`]
//! type. Both are named when the function is made (`Function::new::<A, R>`),
//! so the implementation closure needs no type annotations and may borrow
//! from its arguments.

use std::fmt;
use std::sync::Arc;

use crate::error::{BoxError, Error};
use crate::value::{ArrayRef, Cidr, Ip, Mac, TextForm, Url, Val, Value, ValueRef};

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
/// [`Cidr`], [`Mac`], `&`[`Url`], `&regex::Regex`, [`ArrayRef`] (a borrowed
/// list), [`TextForm`] (a string or any value with a text form), and
/// [`ValueRef`] (any value).
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
from_arg!(ArrayRef<'a>, "array", ValueRef::Array(v) => v);

/// Strings and values with a text form (IPs, CIDRs, MACs, URLs), read as
/// their text.
impl<'a> FromArg<'a> for TextForm<'a> {
    const TYPE: &'static str = "string";
    #[inline(always)]
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

/// Return types of functions.
///
/// Implemented for `bool`, `i64`, `u64`, `f64`, `&'static str`, `String`,
/// `&'static [u8]`, `Vec<u8>`, [`Ip`], [`Cidr`], [`Mac`], [`Url`], and, for
/// values whose type is only known at run time, [`ValueRef`], [`Val`], and
/// [`Value`].
pub trait Returns: 'static {
    /// The type name of returned values (`"any"` when only known at run time).
    const TYPE: &'static str;
    /// Convert a returned value.
    fn into_val(self) -> Val<'static>;
}

macro_rules! returns {
    ($ty:ty, $name:literal, |$v:ident| $out:expr) => {
        impl Returns for $ty {
            const TYPE: &'static str = $name;
            fn into_val(self) -> Val<'static> {
                let $v = self;
                $out
            }
        }
    };
}

returns!(bool, "bool", |v| Val::Ref(ValueRef::Bool(v)));
returns!(i64, "int64", |v| Val::Ref(ValueRef::Int(v)));
returns!(u64, "uint64", |v| Val::Ref(ValueRef::Uint(v)));
returns!(f64, "float64", |v| Val::Ref(ValueRef::Float(v)));
returns!(&'static str, "string", |v| Val::Ref(ValueRef::Str(v)));
returns!(String, "string", |v| Val::Owned(Value::String(v)));
returns!(&'static [u8], "bytes", |v| Val::Ref(ValueRef::Bytes(v)));
returns!(Vec<u8>, "bytes", |v| Val::Owned(Value::Bytes(v)));
returns!(Ip, "ip", |v| Val::Ref(ValueRef::Ip(v)));
returns!(Cidr, "cidr", |v| Val::Ref(ValueRef::Cidr(v)));
returns!(Mac, "mac", |v| Val::Ref(ValueRef::Mac(v)));
returns!(Url, "url", |v| Val::Owned(Value::Url(Box::new(v))));
returns!(ValueRef<'static>, "any", |v| Val::Ref(v));
returns!(Val<'static>, "any", |v| v);
returns!(Value, "any", |v| Val::Owned(v));

/// A function's name and documentation, for [`Function::new`].
///
/// ```rust
/// use rulekit::FuncSchema;
/// let schema = FuncSchema { name: "is_internal", doc: "Whether a host is internal." };
/// ```
///
/// The fields are `&'static str` so a struct literal works with plain string
/// literals; function sets are normally fixed at compile time. A name built at
/// run time can be leaked once at registration (`String::leak`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct FuncSchema {
    /// The name rules call the function by.
    pub name: &'static str,
    /// What the function does, for tools and documentation.
    pub doc: &'static str,
}

/// The arguments of a function that takes none.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct NoArgs;

impl Args for NoArgs {
    type Of<'a> = NoArgs;
    const PARAMS: &'static [Param] = &[];
    #[inline(always)]
    fn parse<'a>(_: &'a [Val<'a>]) -> Result<NoArgs, Error> {
        Ok(NoArgs)
    }
}

/// Why a custom [`Function`] produced no value.
///
/// `?` converts any error type, a [`BoxError`], or a `String`/`&str` message
/// into [`FnError::Error`].
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

impl<E: Into<BoxError>> From<E> for FnError {
    fn from(error: E) -> Self {
        FnError::Error(error.into())
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

type Callable = dyn for<'s> Fn(&'s [Val<'s>]) -> Result<Val<'static>, CallFailure> + Send + Sync;

/// A custom function callable from rules as `name(arg, ...)`.
///
/// Arguments are evaluated first; if one is missing, the call is not made and
/// the result is unknown. The argument count must match the parameters (an
/// [`Error::ArgCount`]); each argument must have its parameter's type (an
/// [`Error::InvalidArg`]). The implementation receives the arguments. It
/// returns a value, missing fields ([`FnError::Missing`]: the result is
/// unknown), or an error ([`FnError::Error`], reported as
/// [`Error::Function`]).
///
/// Functions only see their arguments, not the rule's input; state that
/// outlives one evaluation is captured by the closure. Register them with
/// [`OptsBuilder::function`](crate::OptsBuilder::function).
///
/// ```rust
/// use rulekit::{FuncSchema, Function, NoArgs, NoInput, Opts};
///
/// #[derive(rulekit::Args)]
/// struct ClampArgs {
///     n: i64,
///     max: i64,
/// }
///
/// let clamp = Function::new::<ClampArgs, i64>(
///     FuncSchema { name: "clamp", doc: "The smaller of n and max." },
///     |a| Ok(a.n.min(a.max)),
/// );
/// let tenant_name = String::from("acme");
/// let tenant = Function::new::<NoArgs, String>(
///     FuncSchema { name: "tenant", doc: "The tenant of the request." },
///     move |_| Ok(tenant_name.clone()),
/// );
///
/// let opts = Opts::builder().function(clamp).function(tenant).build()?;
/// let rule = rulekit::parse(r#"clamp(150, 100) == 100 and tenant() == "acme""#)?;
/// assert!(rule.eval(&NoInput, &opts).pass());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub struct Function {
    name: &'static str,
    doc: &'static str,
    params: &'static [Param],
    returns: &'static str,
    call: Arc<Callable>,
}

impl Clone for Function {
    fn clone(&self) -> Self {
        Function {
            name: self.name,
            doc: self.doc,
            params: self.params,
            returns: self.returns,
            call: self.call.clone(),
        }
    }
}

impl fmt::Debug for Function {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Function")
            .field("name", &self.name)
            .field("params", &self.params)
            .field("returns", &self.returns)
            .finish()
    }
}

impl Function {
    /// A function named by `schema`, taking the arguments `A` (a struct
    /// deriving [`Args`](crate::Args), or [`NoArgs`]) and returning `R`,
    /// implemented by `f`: `Function::new::<A, R>(schema, |args| ...)`.
    ///
    /// Argument values live only for the call.
    pub fn new<A: Args, R: Returns>(
        schema: FuncSchema,
        f: impl for<'s> Fn(A::Of<'s>) -> Result<R, FnError> + Send + Sync + 'static,
    ) -> Self {
        Function {
            name: schema.name,
            doc: schema.doc,
            params: A::PARAMS,
            returns: R::TYPE,
            call: Arc::new(move |vals| invoke::<A, R, _>(&f, vals)),
        }
    }

    /// The function name.
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// The documentation.
    pub fn doc(&self) -> &'static str {
        self.doc
    }

    /// The parameters, in order.
    pub fn params(&self) -> &'static [Param] {
        self.params
    }

    /// The return type name (`"any"` when only known at run time).
    pub fn returns(&self) -> &'static str {
        self.returns
    }

    /// Run the function. The caller has checked the argument count.
    pub(crate) fn call(&self, vals: &[Val<'_>]) -> Result<Val<'static>, CallFailure> {
        (self.call)(vals).map_err(|failure| failure.named(self.name))
    }

    /// Check an argument count against the parameters (Go: before the
    /// arguments are evaluated).
    pub(crate) fn check_arity(&self, got: usize) -> Result<(), Error> {
        check_arity(self.name, self.params, got)
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
#[inline(always)]
pub(crate) fn invoke<A, R, F>(f: &F, vals: &[Val<'_>]) -> Result<Val<'static>, CallFailure>
where
    A: Args,
    R: Returns,
    F: for<'s> Fn(A::Of<'s>) -> Result<R, FnError>,
{
    let args = A::parse(vals).map_err(CallFailure::Error)?;
    match f(args) {
        Ok(value) => Ok(value.into_val()),
        Err(FnError::Missing(fields)) => Err(CallFailure::Missing(fields)),
        Err(FnError::Error(source)) => Err(CallFailure::Failed(source)),
    }
}

/// Support code for `#[derive(Args)]` and `#[derive(Input)]`. Not public API.
#[doc(hidden)]
pub mod __private {
    use super::FromArg;
    use crate::error::Error;
    use crate::value::Val;

    /// The argument at `index` as `T`.
    #[inline(always)]
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

    /// Resolve `path` against a derived-input field serde writes. The
    /// turbofish on `T` is spanned at the field type so an unsupported type
    /// points there. An absent `Option` (see
    /// [`InputValue::ABSENT_IS_NULL`](crate::InputValue::ABSENT_IS_NULL)) is
    /// `null`, as serde writes it; a path below it is missing. For other
    /// types this is a plain `get`.
    #[inline(always)]
    pub fn field<'a, T>(
        value: &'a T,
        path: &[crate::ast::Segment],
    ) -> Result<Option<crate::value::Val<'a>>, crate::error::BoxError>
    where
        T: crate::InputValue,
    {
        // Branch on the constant first so other types compile to the bare
        // call, with its result written straight into the caller's slot.
        if !T::ABSENT_IS_NULL || !path.is_empty() {
            return crate::InputValue::get(value, path);
        }
        match crate::InputValue::get(value, path)? {
            None => null(path),
            found => Ok(found),
        }
    }

    /// A struct, map-like variant, or tuple read whole.
    #[inline(always)]
    pub fn object<'a>() -> Result<Option<crate::value::Val<'a>>, crate::error::BoxError> {
        Ok(Some(crate::value::Val::Ref(
            crate::value::ValueRef::Object(crate::value::ObjectRef::Opaque),
        )))
    }

    /// A variant name: a unit variant, or an enum tag.
    #[inline(always)]
    pub fn name<'a>(
        name: &'a str,
        path: &[crate::ast::Segment],
    ) -> Result<Option<crate::value::Val<'a>>, crate::error::BoxError> {
        Ok(crate::input_value::project(
            crate::value::Val::Ref(crate::value::ValueRef::Str(name)),
            path,
        ))
    }

    /// `null`: a unit struct or untagged unit variant.
    #[inline(always)]
    pub fn null<'a>(
        path: &[crate::ast::Segment],
    ) -> Result<Option<crate::value::Val<'a>>, crate::error::BoxError> {
        Ok(crate::input_value::project(
            crate::value::Val::Ref(crate::value::ValueRef::Null),
            path,
        ))
    }

    /// Read a `#[rulekit(bytes)]` field as a byte string. `T` is spanned at
    /// the field type so a non-byte field fails there.
    #[inline(always)]
    pub fn bytes_field<'a, T>(
        value: &'a T,
        path: &[crate::ast::Segment],
    ) -> Result<Option<crate::value::Val<'a>>, crate::error::BoxError>
    where
        T: crate::ByteStr,
    {
        crate::bytes_input::bytes_value(value, path)
    }
}
