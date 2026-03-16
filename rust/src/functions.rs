use std::sync::LazyLock;

use crate::errors::EvalError;
use crate::eval::{EvalResult, Value};

/// Extract a typed value from a runtime `Value`.
pub trait FromValue: Sized {
    fn type_name() -> &'static str;
    fn from_value(val: &Value) -> Option<Self>;
}

impl FromValue for String {
    fn type_name() -> &'static str { "String" }
    fn from_value(val: &Value) -> Option<Self> {
        match val { Value::String(s) => Some(s.clone()), _ => None }
    }
}

impl FromValue for i64 {
    fn type_name() -> &'static str { "Int" }
    fn from_value(val: &Value) -> Option<Self> {
        match val { Value::Int(n) => Some(*n), _ => None }
    }
}

impl FromValue for f64 {
    fn type_name() -> &'static str { "Float" }
    fn from_value(val: &Value) -> Option<Self> {
        match val { Value::Float(n) => Some(*n), _ => None }
    }
}

impl FromValue for bool {
    fn type_name() -> &'static str { "Bool" }
    fn from_value(val: &Value) -> Option<Self> {
        match val { Value::Bool(b) => Some(*b), _ => None }
    }
}

/// Pass-through: accepts any Value without type checking.
impl FromValue for Value {
    fn type_name() -> &'static str { "Value" }
    fn from_value(val: &Value) -> Option<Self> { Some(val.clone()) }
}

/// A typed function handler that can be called with a slice of `Value`s.
/// Implemented for closures of arities 1–3 via macro below.
pub trait Handler<Args>: Send + Sync + 'static {
    fn call(&self, args: &[Value], names: &[&str]) -> EvalResult;
}

macro_rules! impl_handler {
    ($($idx:tt : $T:ident => $var:ident),+) => {
        impl<F, $($T: FromValue),+> Handler<($($T,)+)> for F
        where
            F: Fn($($T),+) -> Value + Send + Sync + 'static,
        {
            fn call(&self, args: &[Value], names: &[&str]) -> EvalResult {
                $(
                    let $var = match $T::from_value(&args[$idx]) {
                        Some(v) => v,
                        None => return EvalResult {
                            value: Value::Null,
                            error: Some(EvalError::InvalidFunctionArg {
                                name: names[$idx].to_string(),
                                expected: $T::type_name().to_string(),
                                got: format!("{}", args[$idx]),
                            }),
                        },
                    };
                )+
                EvalResult { value: (self)($($var),+), error: None }
            }
        }
    };
}

impl_handler!(0: A => a);
impl_handler!(0: A => a, 1: B => b);
impl_handler!(0: A => a, 1: B => b, 2: C => c);

/// Definition of a callable function (stdlib or user-defined).
pub struct FunctionDef {
    pub args: &'static [&'static str],
    eval: Box<dyn Fn(&[Value]) -> EvalResult + Send + Sync>,
}

impl FunctionDef {
    /// Create a function definition from a typed closure.
    ///
    /// ```ignore
    /// FunctionDef::new(&["value", "prefix"], |value: String, prefix: String| {
    ///     Value::Bool(value.starts_with(&prefix))
    /// })
    /// ```
    pub fn new<Args, F: Handler<Args>>(args: &'static [&'static str], f: F) -> Self {
        let names = args;
        FunctionDef {
            args,
            eval: Box::new(move |vals| f.call(vals, names)),
        }
    }

    pub fn call(&self, args: &[Value]) -> EvalResult {
        (self.eval)(args)
    }
}

/// Static registry of stdlib functions.
pub static STDLIB: LazyLock<Vec<(&'static str, FunctionDef)>> = LazyLock::new(|| {
    vec![
        ("starts_with", FunctionDef::new(&["value", "prefix"], |value: String, prefix: String| {
            Value::Bool(value.starts_with(&prefix))
        })),
    ]
});

/// Look up a stdlib function by name.
pub fn get_stdlib(name: &str) -> Option<&'static FunctionDef> {
    STDLIB.iter().find(|(n, _)| *n == name).map(|(_, def)| def)
}
