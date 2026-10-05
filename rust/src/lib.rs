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
mod error;
mod lex;
mod literal;
mod parse;
mod print;
mod regex;
pub mod value;

use std::fmt;
use std::sync::Arc;

pub use ast::Ast;
pub use error::ParseError;
pub use print::{PrintMode, format};

use ast::{NodeData, NodeId};

/// A compiled rule.
#[derive(Clone, Debug)]
pub struct Rule {
    ast: Arc<Ast>,
}

/// Parse and compile an expression (Go `Parse`).
pub fn parse(source: &str) -> Result<Rule, ParseError> {
    compile(Arc::new(Ast::parse(source)?))
}

/// Compile a parsed AST (Go `Compile`). Literal values are parsed here, so
/// invalid literals (for example a malformed regex) are reported as parse
/// errors at the literal.
pub fn compile(ast: Arc<Ast>) -> Result<Rule, ParseError> {
    check_literals(&ast, ast.root().id())?;
    Ok(Rule { ast })
}

/// Parse every literal in evaluation-lowering order; the first error wins.
fn check_literals(ast: &Ast, id: NodeId) -> Result<(), ParseError> {
    match ast.data(id) {
        NodeData::Literal { span, kind } => {
            literal::parse_literal(*kind, ast.text(*span))
                .map_err(|err| ParseError::at(ast.source(), span.start, err))?;
        }
        NodeData::Path { .. } => {}
        NodeData::Array { items: ids, .. } | NodeData::Call { args: ids, .. } => {
            for &child in ids.iter() {
                check_literals(ast, child)?;
            }
        }
        NodeData::Unary { operand, .. } => check_literals(ast, *operand)?,
        NodeData::Binary { lhs, rhs, .. } => {
            check_literals(ast, *lhs)?;
            check_literals(ast, *rhs)?;
        }
    }
    Ok(())
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
