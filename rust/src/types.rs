use crate::ast::{Expr, Operator};

/// Source location of a token.
#[derive(Clone, Copy, Debug, Default)]
pub struct Loc {
    pub begin: usize,
    pub end: usize,
}

/// A token produced by the lexer and consumed by the parser.
#[derive(Clone, Debug)]
pub struct Token {
    pub token_type: i32,
    pub token_value: Vec<u8>,
    pub loc: Loc,
}

impl Token {
    /// Extraction function for `$<Token>N` in parser.y.
    pub fn from(v: Value) -> Self {
        match v {
            Value::Token(t) => t,
            other => panic!("expected Token, got {:?}", other),
        }
    }
}

/// The semantic-value stack type used by rust-bison-skeleton.
///
/// Every `%type <Variant>` and `$<Variant>N` in parser.y refers to one of
/// these variants via an extraction function or module of the same name.
#[derive(Clone, Debug)]
pub enum Value {
    None,
    Uninitialized,
    Stolen,
    Token(Token),
    Node(Expr),
    Op(Operator),
    NodeList(Vec<Expr>),
}

impl Default for Value {
    fn default() -> Self {
        Self::Stolen
    }
}

impl Value {
    pub fn new_uninitialized() -> Self {
        Self::Uninitialized
    }

    pub fn is_uninitialized(&self) -> bool {
        matches!(self, Self::Uninitialized)
    }

    pub fn from_token(token: Token) -> Self {
        Self::Token(token)
    }
}

// --- Extraction modules used by $<Variant>N in parser.y ---

#[allow(non_snake_case)]
pub mod Node {
    use super::Value;
    use crate::ast::Expr;
    pub fn from(v: Value) -> Expr {
        match v {
            Value::Node(e) => e,
            other => panic!("expected Node, got {:?}", other),
        }
    }
}

#[allow(non_snake_case)]
pub mod Op {
    use super::Value;
    use crate::ast::Operator;
    pub fn from(v: Value) -> Operator {
        match v {
            Value::Op(o) => o,
            other => panic!("expected Op, got {:?}", other),
        }
    }
}

#[allow(non_snake_case)]
pub mod NodeList {
    use super::Value;
    use crate::ast::Expr;
    pub fn from(v: Value) -> Vec<Expr> {
        match v {
            Value::NodeList(l) => l,
            other => panic!("expected NodeList, got {:?}", other),
        }
    }
}
