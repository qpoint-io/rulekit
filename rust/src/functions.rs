use std::collections::HashMap;

use crate::eval::{EvalResult, Value};

/// A named function argument.
pub struct FunctionArg {
    pub name: &'static str,
}

/// Definition of a callable function (stdlib or user-defined).
pub struct FunctionDef {
    pub args: &'static [&'static str],
    pub eval: fn(&HashMap<String, Value>) -> EvalResult,
}

/// Static registry of stdlib functions.
pub const STDLIB: &[(&str, FunctionDef)] = &[
    ("starts_with", FunctionDef {
        args: &["value", "prefix"],
        eval: fn_starts_with,
    }),
];

/// Look up a stdlib function by name.
pub fn get_stdlib(name: &str) -> Option<&'static FunctionDef> {
    STDLIB.iter().find(|(n, _)| *n == name).map(|(_, def)| def)
}

/// starts_with(value, prefix) — converts both to string, returns Bool.
fn fn_starts_with(args: &HashMap<String, Value>) -> EvalResult {
    let value = &args["value"];
    let prefix = &args["prefix"];
    let v = value_to_string(value);
    let p = value_to_string(prefix);
    EvalResult { value: Value::Bool(v.starts_with(&p)), error: None }
}

/// Convert a Value to its string representation for function args.
/// Matches Go's fmt.Sprint behavior — unquoted for strings.
fn value_to_string(val: &Value) -> String {
    match val {
        Value::String(s) => s.clone(),
        other => format!("{}", other),
    }
}
