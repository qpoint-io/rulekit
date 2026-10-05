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
//! - Inputs: [`derive(Input)`](Input) on your structs, [`kv!`], a string-keyed
//!   map, or [`serde_json::Value`]. [`KvInput`] remains for owned trees.
//! - [`Env`] holds custom [`Function`]s and [`Macro`]s; [`Opts`] passes it to
//!   evaluation and turns on [`Trace`]s.
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
//! let result = rule.eval(&(), &input, Opts::default());
//! if let Some(err) = result.error() {
//!     panic!("evaluation failed: {err}");
//! } else if result.unknown() {
//!     println!("missing fields: {:?}", result.missing_fields().collect::<Vec<_>>());
//! } else {
//!     assert!(result.pass());
//! }
//! # Ok::<(), rulekit::ParseError>(())
//! ```
//!
//! # Evaluation context
//!
//! [`Rule::eval`] takes a caller-defined context `&C` that is handed to
//! inputs, [`Lazy`] values, and functions (for example a request or tenant).
//! Use `&()` when there is none. The context type is fixed per [`Env`],
//! [`Opts`], and [`KvInput`].

#![warn(missing_docs)]

// Lets `#[derive(Args)]` output (which names `::rulekit`) work in this crate's tests.
extern crate self as rulekit;

pub mod ast;
mod env;
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
mod parse;
mod print;
mod regex;
mod stdlib;
pub mod value;

use std::fmt;
use std::sync::{Arc, LazyLock};

pub use ast::Ast;
pub use env::{Env, EnvBuilder, Macro};
pub use error::{BoxError, Error, ParseError};
pub use eval::EvalResult;
pub use eval::trace::{Diagnostic, DiagnosticCode, Trace, TraceStatus};
#[doc(hidden)]
pub use func::__private;
pub use func::{Args, FnError, FromArg, FuncSchema, Function, NoArgs, Param, Rest, Returns};
pub use input::{FnInput, Input, Kv, KvEntry, KvInput, Lazy, NoInput};
pub use input_value::InputValue;
pub use json_input::{JsonOptions, decode_json};
pub use kv::{KvEnd, KvList, LazyVal, lazy};
pub use print::{Edit, PrintMode, format, rewrite};
/// Derive [`Args`](trait@Args) for a struct of function arguments. See the
/// trait for the rules.
#[cfg(feature = "derive")]
pub use rulekit_macros::Args;
/// Derive [`Input`](trait@Input) for a struct of named fields. See the trait
/// for the rules.
#[cfg(feature = "derive")]
pub use rulekit_macros::Input;

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

/// Options for [`Rule::eval`]: the [`Env`] of custom functions and macros,
/// and whether to record a [`Trace`].
///
/// `Opts::default()` uses an empty environment with tracing off, for rules
/// that call no custom functions or macros and use the `()` context.
///
/// ```rust
/// use rulekit::{Env, Opts};
///
/// let env: Env = Env::new();
/// let opts = Opts::new(&env).with_trace(true);
/// assert!(opts.trace);
/// ```
pub struct Opts<'e, C: ?Sized = ()> {
    /// Custom functions and macros.
    pub env: &'e Env<C>,
    /// Record an evaluation [`Trace`]. Untraced evaluation runs code with no
    /// trace bookkeeping at all.
    pub trace: bool,
}

impl<C: ?Sized> Clone for Opts<'_, C> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<C: ?Sized> Copy for Opts<'_, C> {}

impl<'e, C: ?Sized> Opts<'e, C> {
    /// Options using `env`, with tracing off.
    pub fn new(env: &'e Env<C>) -> Self {
        Opts { env, trace: false }
    }

    /// The same options with tracing on or off.
    pub fn with_trace(self, trace: bool) -> Self {
        Opts { trace, ..self }
    }
}

static EMPTY_ENV: LazyLock<Env> = LazyLock::new(Env::new);

impl Default for Opts<'static> {
    fn default() -> Self {
        Opts {
            env: &EMPTY_ENV,
            trace: false,
        }
    }
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

    /// Evaluate the rule against `input` (Go `Rule.Eval(ctx, input, opts)`).
    ///
    /// `ctx` is the caller's evaluation context, passed to the input, to
    /// [`Lazy`] values, and to custom functions; use `&()` for none. The
    /// result borrows from the rule, input, and context, so values can be
    /// returned without copying.
    ///
    /// Values of incompatible types compare as false (a [`Trace`] records a
    /// [`Diagnostic`] for them). Fields the input lacks are reported by
    /// [`EvalResult::missing_fields`]; input and function failures by
    /// [`EvalResult::error`].
    pub fn eval<'a, C: ?Sized, I: Input<C> + ?Sized>(
        &'a self,
        ctx: &'a C,
        input: &'a I,
        opts: Opts<'a, C>,
    ) -> EvalResult<'a> {
        let scope = eval::Scope {
            input,
            ctx,
            env: opts.env,
        };
        // `--cfg rulekit_size_probe` leaves the traced instantiation out, so
        // its compiled size can be measured (see bench/size.sh).
        eval::run(&self.root, &scope, opts.trace && !cfg!(rulekit_size_probe))
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
