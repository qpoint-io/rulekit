//! Functions, macros, and the validated evaluation environment (port of
//! `functions.go`, `macros.go`, and `Opts.Validate`).

use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;

use crate::error::{BoxError, Error, ParseError};
use crate::value::{Cidr, Ip, Mac, Map, Url, Val, ValueRef};
use crate::{Rule, stdlib_arity};

/// The type a function argument must have, named as in typed JSON.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Type {
    Null,
    Bool,
    Int64,
    Uint64,
    Float64,
    String,
    Bytes,
    Ip,
    Cidr,
    Mac,
    Url,
    Regex,
    Array,
    Object,
}

impl Type {
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

/// A named positional parameter, optionally typed. A typed parameter is
/// checked before the function runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArgSpec {
    pub name: Cow<'static, str>,
    pub ty: Option<Type>,
}

impl ArgSpec {
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        ArgSpec {
            name: name.into(),
            ty: None,
        }
    }

    pub fn typed(name: impl Into<Cow<'static, str>>, ty: Type) -> Self {
        ArgSpec {
            name: name.into(),
            ty: Some(ty),
        }
    }
}

type FunctionImpl<C> =
    dyn for<'a> Fn(&'a C, Args<'_, 'a>) -> Result<Val<'a>, BoxError> + Send + Sync;

/// A custom function callable from rules. Arguments are positional; the
/// specs name them and give their count.
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

    /// Document the return type (for tools; not enforced).
    pub fn returns(mut self, ty: Type) -> Self {
        self.ret = Some(ty);
        self
    }

    pub fn doc(mut self, doc: impl Into<String>) -> Self {
        self.doc = Some(doc.into());
        self
    }

    pub fn args(&self) -> &[ArgSpec] {
        &self.args
    }

    pub fn return_type(&self) -> Option<Type> {
        self.ret
    }

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

/// The evaluated arguments of one call, by position or by spec name.
#[derive(Clone, Copy)]
pub struct Args<'s, 'a> {
    specs: &'s [ArgSpec],
    vals: &'s [Val<'a>],
}

impl<'s, 'a> Args<'s, 'a> {
    pub fn len(&self) -> usize {
        self.vals.len()
    }

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

    /// The argument at `index` as `T`; an `InvalidArg` error if it has
    /// another type.
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

    /// The argument named `name` as `T` (Go `IndexFuncArg`).
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

/// Types an argument can be read as. Conversions are exact: an `int64`
/// argument is not a `u64`.
pub trait FromArg<'a>: Sized {
    const EXPECTED: &'static str;
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

/// A named zero-argument rule, expanded where it is called.
#[derive(Clone, Debug)]
pub struct Macro {
    source: String,
    pub(crate) rule: Rule,
}

impl Macro {
    pub fn new(source: &str) -> Result<Macro, ParseError> {
        Ok(Macro {
            source: source.to_owned(),
            rule: crate::parse(source)?,
        })
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn rule(&self) -> &Rule {
        &self.rule
    }
}

/// Validated custom functions and macros for evaluation.
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

    pub fn builder() -> EnvBuilder<C> {
        EnvBuilder {
            env: Env::default(),
        }
    }

    pub fn function(&self, name: &str) -> Option<&Function<C>> {
        self.functions.get(name)
    }

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

    /// Parse and add a macro (Go `MacroSet.Register`).
    pub fn macro_source(self, name: impl Into<String>, source: &str) -> Result<Self, ParseError> {
        Ok(self.macro_rule(name, Macro::new(source)?))
    }

    /// Validate names (Go `Opts.Validate`): functions and macros must not
    /// shadow the standard library, and a macro must not share a function's
    /// name.
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
