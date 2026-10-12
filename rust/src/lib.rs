//! Rulekit parses and evaluates rule expressions such as
//!
//! ```text
//! domain matches /example\.com$/ and port == 8080
//! ```
//!
//! against input data. The rule language (operators, types, value fields,
//! macros, functions) is described in the
//! [language reference](https://github.com/qpoint-io/rulekit#readme).
//!
//! # Overview
//!
//! - [`parse`] turns source text into a compiled [`Rule`]; [`Ast::parse`] and
//!   [`compile`] split that into two steps for tools that inspect the tree.
//! - [`Rule::eval`] evaluates a rule against an [`Input`] and returns an
//!   [`EvalResult`]: a value, an error, or the fields the input lacked.
//! - Inputs: [`derive(Input)`](Input) on your structs and enums (following
//!   their serde attributes), [`SerdeInput`] over any [`serde::Serialize`]
//!   value, [`kv!`], a string-keyed map, or [`serde_json::Value`]. [`KvInput`]
//!   remains for owned trees.
//! - [`Opts`] holds custom [`Function`]s and [`Macro`]s and turns on
//!   [`Trace`]s; build one with [`Opts::builder`].
//! - [`format`](fn@format), [`Rule::print`], and [`rewrite`] print
//!   expressions.
//!
//! # Example
//!
//! ```rust
//! use rulekit::Opts;
//!
//! #[derive(rulekit::Input)]
//! struct Request<'a> {
//!     domain: &'a str,
//!     port: u16,
//! }
//!
//! let rule = rulekit::parse(r"domain matches /example\.com$/ and port == 8080")?;
//! let input = Request { domain: "example.com", port: 8080 };
//!
//! let opts = Opts::default();
//! let result = rule.eval(&input, &opts);
//! if let Some(err) = result.error() {
//!     panic!("evaluation failed: {err}");
//! } else if result.unknown() {
//!     println!("missing fields: {:?}", result.missing_fields().collect::<Vec<_>>());
//! } else {
//!     assert!(result.pass());
//! }
//! # Ok::<(), rulekit::ParseError>(())
//! ```

#![warn(missing_docs)]

// Lets `#[derive(Args)]` output (which names `::rulekit`) work in this crate's tests.
extern crate self as rulekit;

pub mod ast;
mod bytes_input;
mod error;
pub(crate) mod eval;
mod ext_url;
mod func;
mod input;
mod input_value;
mod json_input;
mod kv;
mod lex;
mod literal;
mod opts;
mod parse;
mod print;
mod regex;
mod serde_input;
mod stdlib;
pub mod value;

use std::fmt;
use std::sync::Arc;

pub use ast::Ast;
pub use bytes_input::{ByteStr, Bytes, bytes};
pub use error::{BoxError, Error, ParseError};
pub use eval::EvalResult;
pub use eval::trace::{Diagnostic, DiagnosticCode, Trace, TraceStatus};
#[doc(hidden)]
pub use func::__private;
pub use func::{Args, FnError, FromArg, FuncSchema, Function, NoArgs, Param, Rest, Returns};
pub use input::{FnInput, Input, Kv, KvEntry, KvInput, Lazy, NoInput};
pub use input_value::InputValue;
pub use json_input::{JsonOptions, decode_json};
pub use kv::{KvEnd, KvList, lazy};
pub use opts::{Macro, Opts, OptsBuilder};
pub use print::{Edit, PrintMode, format, rewrite};
/// Derive [`Args`](trait@Args) for a struct of function arguments. See the
/// trait for the rules.
#[cfg(feature = "derive")]
pub use rulekit_macros::Args;
/// Derive [`Input`](trait@Input) and [`InputValue`] for a struct or enum,
/// following its `#[serde(...)]` attributes: a rule path is the path in the
/// value's `serde_json` form, read from the typed value in place.
///
/// ```rust
/// use rulekit::Opts;
///
/// #[derive(rulekit::Input)]
/// #[serde(untagged)]
/// enum EventRecord {
///     Entity(EntityRecord),
/// }
///
/// #[derive(rulekit::Input)]
/// struct EntityRecord {
///     severity: Severity,
///     #[serde(flatten)]
///     event: LlmEvent,
/// }
///
/// #[derive(rulekit::Input)]
/// #[serde(rename_all = "lowercase")]
/// enum Severity {
///     Info,
///     Warn,
/// }
///
/// #[derive(rulekit::Input)]
/// #[serde(tag = "type", content = "payload")]
/// enum LlmEvent {
///     #[serde(rename = "llm.request")]
///     Request {
///         model: String,
///         #[serde(skip_serializing_if = "Option::is_none")]
///         request_id: Option<String>,
///     },
/// }
///
/// let record = EventRecord::Entity(EntityRecord {
///     severity: Severity::Warn,
///     event: LlmEvent::Request { model: "gpt-x".into(), request_id: None },
/// });
/// // serde_json: {"severity":"warn","type":"llm.request","payload":{"model":"gpt-x"}}
/// let rule = rulekit::parse(
///     r#"severity == "warn" and (starts_with(type, "process.") or payload.model == "gpt-x")"#,
/// )?;
/// let opts = Opts::default();
/// assert!(rule.eval(&record, &opts).pass());
///
/// let rule = rulekit::parse(r#"payload.request_id == "r1""#)?;
/// let result = rule.eval(&record, &opts);
/// assert_eq!(result.missing_fields().collect::<Vec<_>>(), ["payload.request_id"]);
/// # Ok::<(), rulekit::ParseError>(())
/// ```
#[cfg(feature = "derive")]
pub use rulekit_macros::Input;
pub use serde_input::SerdeInput;

/// A compiled rule, ready to evaluate.
///
/// Create one with [`parse`] or [`compile`]. A rule is immutable and can be
/// shared across threads and evaluated concurrently. Its [`Display`]
/// output is the compact canonical expression, without comments.
///
/// [`Display`]: fmt::Display
#[derive(Clone, Debug)]
pub struct Rule {
    ast: Arc<Ast>,
    pub(crate) root: eval::Node,
}

/// Parse and compile an expression.
///
/// Equivalent to [`compile`]`(Arc::new(`[`Ast::parse`]`(source)?))`.
///
/// # Errors
///
/// A [`ParseError`] for invalid syntax or an invalid literal (for example a
/// malformed regex or IP address), with its line and column.
///
/// ```rust
/// let rule = rulekit::parse("port in [80, 443] and not internal")?;
/// assert_eq!(rule.to_string(), "port in [80, 443] and not internal");
///
/// let err = rulekit::parse("port ==").unwrap_err();
/// assert_eq!(err.line(), 1);
/// # Ok::<(), rulekit::ParseError>(())
/// ```
pub fn parse(source: &str) -> Result<Rule, ParseError> {
    compile(Arc::new(Ast::parse(source)?))
}

/// Compile a parsed [`Ast`] into a [`Rule`].
///
/// # Errors
///
/// Literal values are parsed here, so an invalid literal (for example a
/// malformed regex) is reported as a [`ParseError`] at the literal.
///
/// ```rust
/// use std::sync::Arc;
/// use rulekit::Ast;
///
/// let ast = Arc::new(Ast::parse(r#"request.headers["user-agent"] == "curl""#)?);
/// assert_eq!(ast.root().children().len(), 2);
/// let rule = rulekit::compile(ast)?;
/// # let _ = rule;
/// # Ok::<(), rulekit::ParseError>(())
/// ```
pub fn compile(ast: Arc<Ast>) -> Result<Rule, ParseError> {
    let root = eval::lower(&ast, ast.root().id())?;
    Ok(Rule { ast, root })
}

impl Rule {
    /// The AST the rule was compiled from.
    pub fn ast(&self) -> &Ast {
        &self.ast
    }

    /// Print the rule's expression in `mode`; the same as
    /// [`format`](fn@format)`(rule.ast(), mode)`.
    pub fn print(&self, mode: &PrintMode) -> String {
        format(&self.ast, mode)
    }

    /// Evaluate the rule against `input` (Go `Rule.Eval(input, opts)`).
    ///
    /// The result borrows from the rule, input, and `opts`, so values can be
    /// returned without copying; keep `opts` alive as long as the result.
    ///
    /// Values of incompatible types compare as false (a [`Trace`] records a
    /// [`Diagnostic`] for them). Fields the input lacks are reported by
    /// [`EvalResult::missing_fields`]; input and function failures by
    /// [`EvalResult::error`].
    pub fn eval<'a, I: Input + ?Sized>(&'a self, input: &'a I, opts: &'a Opts) -> EvalResult<'a> {
        let scope = eval::Scope { input, opts };
        // `--cfg rulekit_size_probe` leaves the traced instantiation out, so
        // its compiled size can be measured (see bench/size.sh).
        eval::run(
            &self.root,
            &scope,
            opts.trace() && !cfg!(rulekit_size_probe),
        )
    }
}

/// The compact canonical expression (comments dropped).
impl fmt::Display for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&*self.ast, f)
    }
}

/// Argument count of a standard library function, checked at parse time.
pub(crate) fn stdlib_arity(name: &str) -> Option<usize> {
    stdlib::params(name).map(<[func::Param]>::len)
}

/// Internal hooks for the crate's benchmarks. Not part of the API.
#[doc(hidden)]
pub mod __bench {
    pub use crate::eval::cmp_number;
}
