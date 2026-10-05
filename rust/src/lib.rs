//! Rulekit parses and evaluates rule expressions such as
//!
//! ```text
//! domain matches /example\.com$/ and port == 8080
//! ```
//!
//! against input values. This is the Rust implementation of
//! <https://github.com/qpoint-io/rulekit>; see its README for the language
//! reference.

pub mod ast;
mod env;
mod error;
mod eval;
mod input;
mod json_input;
mod lex;
mod literal;
mod parse;
mod print;
mod regex;
pub mod value;

use std::fmt;
use std::sync::{Arc, LazyLock};

pub use ast::Ast;
pub use env::{ArgSpec, Args, Env, EnvBuilder, FromArg, Function, Macro, Type};
pub use error::{BoxError, Error, ParseError};
pub use eval::EvalResult;
pub use input::{FnInput, Input, Kv, KvEntry, KvInput, NoInput};
pub use json_input::{JsonOptions, decode_json};
pub use print::{PrintMode, format};

/// A compiled rule.
#[derive(Clone, Debug)]
pub struct Rule {
    ast: Arc<Ast>,
    pub(crate) root: eval::Node,
}

/// Parse and compile an expression (Go `Parse`).
pub fn parse(source: &str) -> Result<Rule, ParseError> {
    compile(Arc::new(Ast::parse(source)?))
}

/// Compile a parsed AST (Go `Compile`). Literal values are parsed here, so
/// invalid literals (for example a malformed regex) are reported as parse
/// errors at the literal.
pub fn compile(ast: Arc<Ast>) -> Result<Rule, ParseError> {
    let root = eval::lower(&ast, ast.root().id())?;
    Ok(Rule { ast, root })
}

/// Evaluation options.
pub struct Opts<'e, C: ?Sized = ()> {
    /// Custom functions and macros.
    pub env: &'e Env<C>,
}

impl<C: ?Sized> Clone for Opts<'_, C> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<C: ?Sized> Copy for Opts<'_, C> {}

impl<'e, C: ?Sized> Opts<'e, C> {
    pub fn new(env: &'e Env<C>) -> Self {
        Opts { env }
    }
}

static EMPTY_ENV: LazyLock<Env> = LazyLock::new(Env::new);

impl Default for Opts<'static> {
    fn default() -> Self {
        Opts { env: &EMPTY_ENV }
    }
}

impl Rule {
    /// The AST the rule was compiled from.
    pub fn ast(&self) -> &Arc<Ast> {
        &self.ast
    }

    /// Print the rule in the given mode.
    pub fn print(&self, mode: &PrintMode) -> String {
        format(&self.ast, mode)
    }

    /// Evaluate the rule against `input`. `ctx` is passed to inputs and
    /// functions.
    pub fn eval<'a, C: ?Sized, I: Input<C> + ?Sized>(
        &'a self,
        input: &'a I,
        ctx: &'a C,
        opts: Opts<'a, C>,
    ) -> EvalResult<'a> {
        self.root.eval(&eval::Scope {
            input,
            ctx,
            env: opts.env,
        })
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
    match name {
        "starts_with" => Some(2),
        _ => None,
    }
}
