//! Functions, macros, and the validated evaluation environment.

use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;

use crate::error::{BoxError, Error, ParseError};
use crate::value::{Cidr, Ip, Mac, Map, Url, Val, ValueRef};
use crate::{Rule, stdlib_arity};

/// A value type, used to declare function arguments and return types.
///
/// The names ([`Type::name`]) are the typed-JSON type names, the same names
/// [`ValueRef::type_name`] returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Type {
    /// `null`.
    Null,
    /// `bool`.
    Bool,
    /// `int64`: [`ValueRef::Int`].
    Int64,
    /// `uint64`: [`ValueRef::Uint`].
    Uint64,
    /// `float64`: [`ValueRef::Float`].
    Float64,
    /// `string`: [`ValueRef::Str`] and [`ValueRef::Query`].
    String,
    /// `bytes`, including hex literals.
    Bytes,
    /// `ip`.
    Ip,
    /// `cidr`.
    Cidr,
    /// `mac`.
    Mac,
    /// `url`.
    Url,
    /// `regex`.
    Regex,
    /// `array`.
    Array,
    /// `object`.
    Object,
}

impl Type {
    /// The type name: `null`, `bool`, `int64`, `uint64`, `float64`,
    /// `string`, `bytes`, `ip`, `cidr`, `mac`, `url`, `regex`, `array`, or
    /// `object`.
    pub fn name(self) -> &'static str {
        match self {
            Type::Null => "null",
            Type::Bool => "bool",
            Type::Int64 => "int64",
            Type::Uint64 => "uint64",
            Type::Float64 => "float64",
            Type::String => "string",
            Type::Bytes => "bytes",
            Type::Ip => "ip",
            Type::Cidr => "cidr",
            Type::Mac => "mac",
            Type::Url => "url",
            Type::Regex => "regex",
            Type::Array => "array",
            Type::Object => "object",
        }
    }

    fn admits(self, value: ValueRef<'_>) -> bool {
        value.type_name() == self.name()
    }
}

/// A named positional parameter of a [`Function`], optionally typed.
///
/// A typed parameter is checked before the function runs; a mismatch is an
/// [`Error::InvalidArg`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArgSpec {
    /// The parameter name, used by [`Args::by_name`] and in errors.
    pub name: Cow<'static, str>,
    /// The required type, or `None` to accept any value.
    pub ty: Option<Type>,
}

impl ArgSpec {
    /// An untyped parameter.
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        ArgSpec {
            name: name.into(),
            ty: None,
        }
    }

    /// A parameter that must have type `ty`.
    pub fn typed(name: impl Into<Cow<'static, str>>, ty: Type) -> Self {
        ArgSpec {
            name: name.into(),
            ty: Some(ty),
        }
    }
}

type FunctionImpl<C> =
    dyn for<'a> Fn(&'a C, Args<'_, 'a>) -> Result<Val<'a>, BoxError> + Send + Sync;

/// A custom function callable from rules as `name(arg, ...)`.
///
/// The [`ArgSpec`]s name the positional parameters and fix their count; a
/// call with another count is an [`Error::ArgCount`]. Arguments are
/// evaluated first: if one is missing, the call is not made and the result
/// is unknown. The implementation receives the evaluation context and the
/// [`Args`], and returns a value or an error (reported as
/// [`Error::Function`]).
///
/// Register functions in an [`Env`] with [`EnvBuilder::function`].
///
/// ```rust
/// use rulekit::value::{Val, Value, ValueRef};
/// use rulekit::{ArgSpec, Env, Function, NoInput, Opts, Type};
///
/// struct Ctx {
///     tenant: String,
/// }
///
/// // A function reading its arguments.
/// let clamp = Function::new(
///     [ArgSpec::typed("n", Type::Int64), ArgSpec::typed("max", Type::Int64)],
///     |_: &Ctx, args| {
///         let n: i64 = args.index(0)?;
///         let max: i64 = args.by_name("max")?;
///         Ok(Val::Owned(Value::Int(n.min(max))))
///     },
/// )
/// .returns(Type::Int64)
/// .doc("The smaller of n and max.");
///
/// // A function returning data borrowed from the context.
/// let tenant = Function::new([], |ctx: &Ctx, _| Ok(Val::Ref(ValueRef::Str(&ctx.tenant))));
///
/// let env = Env::builder()
///     .function("clamp", clamp)
///     .function("tenant", tenant)
///     .build()?;
///
/// let rule = rulekit::parse(r#"clamp(150, 100) == 100 and tenant() == "acme""#)?;
/// let ctx = Ctx { tenant: "acme".into() };
/// assert!(rule.eval(&NoInput, &ctx, Opts::new(&env)).pass());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub struct Function<C: ?Sized = ()> {
    args: Box<[ArgSpec]>,
    ret: Option<Type>,
    doc: Option<String>,
    eval: Arc<FunctionImpl<C>>,
}

impl<C: ?Sized> Clone for Function<C> {
    fn clone(&self) -> Self {
        Function {
            args: self.args.clone(),
            ret: self.ret,
            doc: self.doc.clone(),
            eval: self.eval.clone(),
        }
    }
}

impl<C: ?Sized> fmt::Debug for Function<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Function")
            .field("args", &self.args)
            .field("ret", &self.ret)
            .field("doc", &self.doc)
            .finish()
    }
}

impl<C: ?Sized> Function<C> {
    /// A function with the given parameters and implementation.
    ///
    /// The result may borrow from the context (`&'a C`); values read from
    /// [`Args`] are only borrowed for the call, so return them as
    /// [`Val::Owned`] (for example with [`ValueRef::to_owned`]).
    pub fn new<F>(args: impl IntoIterator<Item = ArgSpec>, eval: F) -> Self
    where
        F: for<'a> Fn(&'a C, Args<'_, 'a>) -> Result<Val<'a>, BoxError> + Send + Sync + 'static,
    {
        Function {
            args: args.into_iter().collect(),
            ret: None,
            doc: None,
            eval: Arc::new(eval),
        }
    }

    /// Declare the return type. Informational (for tools and
    /// documentation); it is not checked.
    pub fn returns(mut self, ty: Type) -> Self {
        self.ret = Some(ty);
        self
    }

    /// Attach a description, for tools and documentation.
    pub fn doc(mut self, doc: impl Into<String>) -> Self {
        self.doc = Some(doc.into());
        self
    }

    /// The declared parameters.
    pub fn args(&self) -> &[ArgSpec] {
        &self.args
    }

    /// The return type set with [`returns`](Self::returns).
    pub fn return_type(&self) -> Option<Type> {
        self.ret
    }

    /// The description set with [`doc`](Self::doc).
    pub fn documentation(&self) -> Option<&str> {
        self.doc.as_deref()
    }

    /// Check argument types and run the function.
    pub(crate) fn call<'a>(
        &self,
        name: &str,
        ctx: &'a C,
        vals: &[Val<'a>],
    ) -> Result<Val<'a>, Error> {
        for (spec, val) in self.args.iter().zip(vals) {
            if let Some(ty) = spec.ty
                && !ty.admits(val.as_ref())
            {
                return Err(Error::InvalidArg {
                    name: spec.name.to_string(),
                    expected: ty.name().to_owned(),
                    got: val.as_ref().type_name().to_owned(),
                });
            }
        }
        (self.eval)(
            ctx,
            Args {
                specs: &self.args,
                vals,
            },
        )
        .map_err(|source| Error::Function {
            name: name.to_owned(),
            source,
        })
    }
}

/// The evaluated arguments of one [`Function`] call, read by position or by
/// [`ArgSpec`] name.
#[derive(Clone, Copy)]
pub struct Args<'s, 'a> {
    specs: &'s [ArgSpec],
    vals: &'s [Val<'a>],
}

impl<'s, 'a> Args<'s, 'a> {
    /// The number of arguments.
    pub fn len(&self) -> usize {
        self.vals.len()
    }

    /// Whether the call has no arguments.
    pub fn is_empty(&self) -> bool {
        self.vals.is_empty()
    }

    /// The argument at `index`.
    pub fn get(&self, index: usize) -> Option<ValueRef<'s>> {
        self.vals.get(index).map(Val::as_ref)
    }

    /// The argument whose spec has `name`.
    pub fn named(&self, name: &str) -> Option<ValueRef<'s>> {
        self.specs
            .iter()
            .position(|spec| spec.name == name)
            .and_then(|i| self.get(i))
    }

    /// The argument at `index` as `T`.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArg`] if there is no such argument or it has another
    /// type.
    pub fn index<T: FromArg<'s>>(&self, index: usize) -> Result<T, Error> {
        let name = self
            .specs
            .get(index)
            .map_or_else(|| index.to_string(), |spec| spec.name.to_string());
        let value = self.get(index).ok_or_else(|| Error::InvalidArg {
            name: name.clone(),
            expected: T::EXPECTED.to_owned(),
            got: "nothing".to_owned(),
        })?;
        convert(name, value)
    }

    /// The argument named `name` as `T`.
    ///
    /// # Errors
    ///
    /// [`Error::UnknownArg`] if no [`ArgSpec`] has that name;
    /// [`Error::InvalidArg`] if the argument has another type.
    pub fn by_name<T: FromArg<'s>>(&self, name: &str) -> Result<T, Error> {
        let value = self
            .named(name)
            .ok_or_else(|| Error::UnknownArg(name.to_owned()))?;
        convert(name.to_owned(), value)
    }
}

fn convert<'s, T: FromArg<'s>>(name: String, value: ValueRef<'s>) -> Result<T, Error> {
    T::from_arg(value).ok_or_else(|| Error::InvalidArg {
        name,
        expected: T::EXPECTED.to_owned(),
        got: value.type_name().to_owned(),
    })
}

/// Types an argument can be read as with [`Args::index`] and
/// [`Args::by_name`].
///
/// Implemented for `bool`, `i64`, `u64`, `f64`, `&str`, `&[u8]`, [`Ip`],
/// [`Cidr`], [`Mac`], `&`[`Url`], `&regex::Regex`, and [`ValueRef`] (any
/// value). Conversions are exact: an `int64` argument is not a `u64`, and a
/// URL is not a `&str`.
pub trait FromArg<'a>: Sized {
    /// The type name used in [`Error::InvalidArg`] when conversion fails.
    const EXPECTED: &'static str;
    /// Convert `value`, or `None` if it has another type.
    fn from_arg(value: ValueRef<'a>) -> Option<Self>;
}

macro_rules! from_arg {
    ($ty:ty, $name:literal, $pat:pat => $out:expr) => {
        impl<'a> FromArg<'a> for $ty {
            const EXPECTED: &'static str = $name;
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

impl<'a> FromArg<'a> for ValueRef<'a> {
    const EXPECTED: &'static str = "any";
    fn from_arg(value: ValueRef<'a>) -> Option<Self> {
        Some(value)
    }
}

/// A named, zero-argument rule, expanded where it is called as `name()`.
///
/// A macro is evaluated against the same input, context, and [`Env`] as the
/// rule calling it. Calling it with arguments is an [`Error::MacroArgs`].
/// Register macros with [`EnvBuilder::macro_source`] or
/// [`EnvBuilder::macro_rule`].
#[derive(Clone, Debug)]
pub struct Macro {
    source: String,
    pub(crate) rule: Rule,
}

impl Macro {
    /// Parse a macro body.
    ///
    /// # Errors
    ///
    /// A [`ParseError`] if `source` is not a valid expression.
    pub fn new(source: &str) -> Result<Macro, ParseError> {
        Ok(Macro {
            source: source.to_owned(),
            rule: crate::parse(source)?,
        })
    }

    /// The macro body as written.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The compiled macro body.
    pub fn rule(&self) -> &Rule {
        &self.rule
    }
}

/// Validated custom functions and macros, passed to evaluation through
/// [`Opts`](crate::Opts).
///
/// Build one with [`Env::builder`]. `C` is the evaluation context type the
/// functions receive.
///
/// ```rust
/// use rulekit::value::{Map, Value};
/// use rulekit::{Env, KvInput, Opts};
///
/// let env: Env = Env::builder()
///     .macro_source("is_internal", r"ip in 10.0.0.0/8 or domain matches /\.internal$/")?
///     .build()?;
///
/// let rule = rulekit::parse(r#"is_internal() and user != "root""#)?;
/// let input = KvInput::from_values(Map::from_iter([
///     ("domain".to_owned(), Value::String("api.internal".into())),
///     ("user".to_owned(), Value::String("alice".into())),
/// ]));
/// assert!(rule.eval(&input, &(), Opts::new(&env)).pass());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub struct Env<C: ?Sized = ()> {
    pub(crate) functions: Map<Function<C>>,
    pub(crate) macros: Map<Macro>,
}

impl<C: ?Sized> fmt::Debug for Env<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Env")
            .field("functions", &self.functions)
            .field("macros", &self.macros)
            .finish()
    }
}

impl<C: ?Sized> Default for Env<C> {
    fn default() -> Self {
        Env {
            functions: Map::default(),
            macros: Map::default(),
        }
    }
}

impl<C: ?Sized> Env<C> {
    /// An environment with no custom functions or macros.
    pub fn new() -> Self {
        Self::default()
    }

    /// Start building an environment.
    pub fn builder() -> EnvBuilder<C> {
        EnvBuilder {
            env: Env::default(),
        }
    }

    /// The custom function named `name`.
    pub fn function(&self, name: &str) -> Option<&Function<C>> {
        self.functions.get(name)
    }

    /// The macro named `name`.
    pub fn macro_(&self, name: &str) -> Option<&Macro> {
        self.macros.get(name)
    }
}

/// Builds an [`Env`]; names are checked once in [`EnvBuilder::build`].
pub struct EnvBuilder<C: ?Sized = ()> {
    env: Env<C>,
}

impl<C: ?Sized> EnvBuilder<C> {
    /// Add (or replace) a custom function.
    pub fn function(mut self, name: impl Into<String>, function: Function<C>) -> Self {
        self.env.functions.insert(name.into(), function);
        self
    }

    /// Add (or replace) a macro.
    pub fn macro_rule(mut self, name: impl Into<String>, macro_: Macro) -> Self {
        self.env.macros.insert(name.into(), macro_);
        self
    }

    /// Parse `source` and add (or replace) it as a macro.
    ///
    /// # Errors
    ///
    /// A [`ParseError`] if `source` is not a valid expression.
    pub fn macro_source(self, name: impl Into<String>, source: &str) -> Result<Self, ParseError> {
        Ok(self.macro_rule(name, Macro::new(source)?))
    }

    /// Validate and return the environment.
    ///
    /// # Errors
    ///
    /// [`Error::Env`] if a function or macro has the name of a standard
    /// library function (such as `starts_with`), or a macro has the name of
    /// a custom function.
    pub fn build(self) -> Result<Env<C>, Error> {
        let mut names: Vec<&String> = self.env.functions.keys().collect();
        names.sort();
        for name in names {
            if stdlib_arity(name).is_some() {
                return Err(Error::Env(format!(
                    "function {name:?}: name conflicts with a stdlib function"
                )));
            }
        }
        let mut names: Vec<&String> = self.env.macros.keys().collect();
        names.sort();
        for name in names {
            if stdlib_arity(name).is_some() {
                return Err(Error::Env(format!(
                    "macro {name:?}: name conflicts with a stdlib function"
                )));
            }
            if self.env.functions.contains_key(name) {
                return Err(Error::Env(format!(
                    "macro {name:?}: name conflicts with a custom function"
                )));
            }
        }
        Ok(self.env)
    }
}
