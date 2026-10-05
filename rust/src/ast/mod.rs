//! Parsed expression trees, for tools that inspect or edit rules.
//!
//! An [`Ast`] owns its source text, its nodes, and the lossless token stream
//! (with whitespace and comments as trivia). Nodes are addressed by
//! [`NodeId`] and inspected through the read-only [`NodeRef`] view; the tree
//! cannot be modified. To change an expression, parse replacement source and
//! use [`rewrite`](crate::rewrite).
//!
//! ```rust
//! use rulekit::Ast;
//! use rulekit::ast::{AstKind, Operator, Segment};
//!
//! let ast = Ast::parse(r#"request.headers["user-agent"] matches /curl/"#)?;
//! let root = ast.root();
//! assert_eq!(root.kind(), AstKind::Binary);
//! assert_eq!(root.operator(), Some(Operator::Matches));
//! assert_eq!(root.raw_operator(), Some("matches"));
//!
//! let lhs = root.children()[0];
//! let path = lhs.path().unwrap();
//! assert_eq!(path[2], Segment::Key { key: "user-agent".into(), bracket: true });
//! assert_eq!(ast.to_string(), r#"request.headers["user-agent"] =~ /curl/"#);
//! # Ok::<(), rulekit::ParseError>(())
//! ```

mod json;

use std::fmt;

pub use json::{JsonAst, JsonNode, JsonSpan, JsonToken};

use crate::error::ParseError;

/// A byte range in the expression source.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Span {
    /// Offset of the first byte.
    pub start: usize,
    /// Offset one past the last byte.
    pub end: usize,
}

impl Span {
    pub(crate) const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// Go `joinSpan`: an all-zero span is treated as absent.
    pub(crate) fn join(self, right: Span) -> Span {
        if self.start == 0 && self.end == 0 {
            return right;
        }
        if right.start == 0 && right.end == 0 {
            return self;
        }
        Span::new(self.start, right.end)
    }
}

/// Lexical token kinds. [`TokenKind::name`] gives the kind names used in the
/// JSON AST.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TokenKind {
    /// End of input; always the last token.
    Eof,
    /// A field name, such as `a` or `a.b-c` (dots included).
    Field,
    /// A quoted string.
    String,
    /// Hex bytes: `x"0a0b"` or colon-separated pairs such as `0a:0b`.
    HexString,
    /// An integer literal.
    Int,
    /// A float literal.
    Float,
    /// `true` or `false`.
    Bool,
    /// A CIDR literal.
    IpCidr,
    /// An IP address literal.
    Ip,
    /// A regex literal, `/.../` or `|...|`, with flags.
    Regex,
    /// `(`.
    LParen,
    /// `)`.
    RParen,
    /// `[`.
    LBracket,
    /// `]`.
    RBracket,
    /// `.`.
    Dot,
    /// `,`.
    Comma,
    /// `not` or `!`.
    Not,
    /// `and` or `&&`.
    And,
    /// `or` or `||`.
    Or,
    /// `==` or `eq`.
    Eq,
    /// `!=` or `ne`.
    Ne,
    /// `>` or `gt`.
    Gt,
    /// `>=` or `ge`.
    Ge,
    /// `<` or `lt`.
    Lt,
    /// `<=` or `le`.
    Le,
    /// `contains`.
    Contains,
    /// `matches` or `=~`.
    Matches,
    /// `in`.
    In,
}

impl TokenKind {
    /// The token kind name used by the token stream and the JSON AST.
    pub fn name(self) -> &'static str {
        match self {
            TokenKind::Eof => "EOF",
            TokenKind::Field => "FIELD",
            TokenKind::String => "STRING",
            TokenKind::HexString => "HEX_STRING",
            TokenKind::Int => "INT",
            TokenKind::Float => "FLOAT",
            TokenKind::Bool => "BOOL",
            TokenKind::IpCidr => "IP_CIDR",
            TokenKind::Ip => "IP",
            TokenKind::Regex => "REGEX",
            TokenKind::LParen => "LPAREN",
            TokenKind::RParen => "RPAREN",
            TokenKind::LBracket => "LBRACKET",
            TokenKind::RBracket => "RBRACKET",
            TokenKind::Dot => "DOT",
            TokenKind::Comma => "COMMA",
            TokenKind::Not => "NOT",
            TokenKind::And => "AND",
            TokenKind::Or => "OR",
            TokenKind::Eq => "EQ",
            TokenKind::Ne => "NE",
            TokenKind::Gt => "GT",
            TokenKind::Ge => "GE",
            TokenKind::Lt => "LT",
            TokenKind::Le => "LE",
            TokenKind::Contains => "CONTAINS",
            TokenKind::Matches => "MATCHES",
            TokenKind::In => "IN",
        }
    }

    /// Syntax-highlighting role: `id`, `str`, `num`, `kw`, or `pun`.
    pub fn role(self) -> &'static str {
        match self {
            TokenKind::Field => "id",
            TokenKind::String
            | TokenKind::Regex
            | TokenKind::Ip
            | TokenKind::IpCidr
            | TokenKind::HexString
            | TokenKind::Bool => "str",
            TokenKind::Int | TokenKind::Float => "num",
            TokenKind::And
            | TokenKind::Or
            | TokenKind::Not
            | TokenKind::Eq
            | TokenKind::Ne
            | TokenKind::Gt
            | TokenKind::Ge
            | TokenKind::Lt
            | TokenKind::Le
            | TokenKind::Contains
            | TokenKind::Matches
            | TokenKind::In => "kw",
            _ => "pun",
        }
    }

    pub(crate) fn operator(self) -> Option<Operator> {
        Some(match self {
            TokenKind::Not => Operator::Not,
            TokenKind::And => Operator::And,
            TokenKind::Or => Operator::Or,
            TokenKind::Eq => Operator::Eq,
            TokenKind::Ne => Operator::Ne,
            TokenKind::Gt => Operator::Gt,
            TokenKind::Ge => Operator::Ge,
            TokenKind::Lt => Operator::Lt,
            TokenKind::Le => Operator::Le,
            TokenKind::Contains => Operator::Contains,
            TokenKind::Matches => Operator::Matches,
            TokenKind::In => Operator::In,
            _ => return None,
        })
    }
}

/// A token with its byte span and the whitespace/comment trivia around it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Token {
    pub kind: TokenKind,
    pub span: Span,
    pub leading: Span,
    pub trailing: Span,
}

/// Read-only view of one token of an [`Ast`].
#[derive(Clone, Copy, Debug)]
pub struct TokenRef<'a> {
    source: &'a str,
    token: &'a Token,
}

impl<'a> TokenRef<'a> {
    /// The token kind.
    pub fn kind(&self) -> TokenKind {
        self.token.kind
    }
    /// The token's byte range in the source.
    pub fn span(&self) -> Span {
        self.token.span
    }
    /// The token text as written (empty for EOF).
    pub fn raw(&self) -> &'a str {
        &self.source[self.token.span.start..self.token.span.end]
    }
    /// Whitespace and comments before the token.
    pub fn leading_trivia(&self) -> &'a str {
        &self.source[self.token.leading.start..self.token.leading.end]
    }
    /// Whitespace and comments after the token (the next token's leading trivia).
    pub fn trailing_trivia(&self) -> &'a str {
        &self.source[self.token.trailing.start..self.token.trailing.end]
    }
}

/// The shape of an AST node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AstKind {
    /// A literal value, such as `1`, `"a"`, or `10.0.0.0/8`.
    Literal,
    /// A field path, such as `a.b[0]`.
    Path,
    /// An array, `[...]`.
    Array,
    /// A function or macro call, `name(...)`.
    Call,
    /// `not` applied to an operand.
    Unary,
    /// A logical or comparison operator with two operands.
    Binary,
}

impl AstKind {
    /// JSON kind name: `literal`, `path`, `array`, `call`, `unary`, `binary`.
    pub fn name(self) -> &'static str {
        match self {
            AstKind::Literal => "literal",
            AstKind::Path => "path",
            AstKind::Array => "array",
            AstKind::Call => "call",
            AstKind::Unary => "unary",
            AstKind::Binary => "binary",
        }
    }
}

/// Normalized unary and binary operators.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Operator {
    /// `not`.
    Not,
    /// `and`.
    And,
    /// `or`.
    Or,
    /// `==`.
    Eq,
    /// `!=`.
    Ne,
    /// `>`.
    Gt,
    /// `>=`.
    Ge,
    /// `<`.
    Lt,
    /// `<=`.
    Le,
    /// `contains`.
    Contains,
    /// `matches` (printed as `=~`).
    Matches,
    /// `in`.
    In,
}

impl Operator {
    /// Machine name used in JSON and diagnostics (`eq`, `contains`, ...).
    pub fn name(self) -> &'static str {
        match self {
            Operator::Not => "not",
            Operator::And => "and",
            Operator::Or => "or",
            Operator::Eq => "eq",
            Operator::Ne => "ne",
            Operator::Gt => "gt",
            Operator::Ge => "ge",
            Operator::Lt => "lt",
            Operator::Le => "le",
            Operator::Contains => "contains",
            Operator::Matches => "matches",
            Operator::In => "in",
        }
    }

    /// Human spelling used by printed rules (`==`, `=~`, ...).
    pub fn symbol(self) -> &'static str {
        match self {
            Operator::Not => "not",
            Operator::And => "and",
            Operator::Or => "or",
            Operator::Eq => "==",
            Operator::Ne => "!=",
            Operator::Gt => ">",
            Operator::Ge => ">=",
            Operator::Lt => "<",
            Operator::Le => "<=",
            Operator::Contains => "contains",
            Operator::Matches => "=~",
            Operator::In => "in",
        }
    }

    /// Go `infixPrecedence`.
    pub(crate) fn precedence(self) -> u8 {
        match self {
            Operator::Or => 1,
            Operator::And => 2,
            Operator::Not => 4,
            _ => 3,
        }
    }
}

/// Literal token kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LiteralKind {
    /// A quoted string.
    String,
    /// Hex bytes.
    HexString,
    /// An integer.
    Int,
    /// A float.
    Float,
    /// `true` or `false`.
    Bool,
    /// An IP address.
    Ip,
    /// A CIDR block.
    Cidr,
    /// A regex.
    Regex,
}

impl LiteralKind {
    pub(crate) fn from_token(kind: TokenKind) -> Option<Self> {
        Some(match kind {
            TokenKind::String => LiteralKind::String,
            TokenKind::HexString => LiteralKind::HexString,
            TokenKind::Int => LiteralKind::Int,
            TokenKind::Float => LiteralKind::Float,
            TokenKind::Bool => LiteralKind::Bool,
            TokenKind::Ip => LiteralKind::Ip,
            TokenKind::IpCidr => LiteralKind::Cidr,
            TokenKind::Regex => LiteralKind::Regex,
            _ => return None,
        })
    }
}

/// One step of a path: a map key or an array index.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Segment {
    /// A map key. `bracket` records `a["b"]` rather than `a.b`.
    Key {
        /// The key.
        key: String,
        /// Whether the key was written in brackets.
        bracket: bool,
    },
    /// An array index, always written with brackets.
    Index(usize),
}

/// Identifies a node within its [`Ast`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(u32);

impl NodeId {
    pub(crate) fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Debug)]
pub(crate) enum NodeData {
    Literal {
        span: Span,
        kind: LiteralKind,
    },
    Path {
        span: Span,
        segments: Box<[Segment]>,
    },
    Array {
        span: Span,
        items: Box<[NodeId]>,
    },
    Call {
        span: Span,
        name: Span,
        args: Box<[NodeId]>,
    },
    Unary {
        span: Span,
        op: Operator,
        raw_op: Span,
        operand: NodeId,
    },
    Binary {
        span: Span,
        op: Operator,
        raw_op: Span,
        negated: bool,
        lhs: NodeId,
        rhs: NodeId,
    },
}

impl NodeData {
    pub(crate) fn span(&self) -> Span {
        match self {
            NodeData::Literal { span, .. }
            | NodeData::Path { span, .. }
            | NodeData::Array { span, .. }
            | NodeData::Call { span, .. }
            | NodeData::Unary { span, .. }
            | NodeData::Binary { span, .. } => *span,
        }
    }
}

/// A parsed expression: source text, nodes, and token stream.
///
/// Its [`Display`](fmt::Display) output is the compact canonical expression
/// without comments; use [`format`](crate::format) for other modes. With
/// serde, an `Ast` serializes as its [`json`](Self::json) document.
#[derive(Clone, Debug)]
pub struct Ast {
    pub(crate) source: String,
    pub(crate) nodes: Vec<NodeData>,
    pub(crate) root: NodeId,
    pub(crate) tokens: Vec<Token>,
}

impl Ast {
    /// Parse an expression without compiling it.
    ///
    /// # Errors
    ///
    /// A [`ParseError`] for invalid syntax. Literal values (such as regexes)
    /// are only checked by [`compile`](crate::compile).
    pub fn parse(source: &str) -> Result<Ast, ParseError> {
        crate::parse::parse(source)
    }

    /// The expression text the AST was parsed from.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The root node.
    pub fn root(&self) -> NodeRef<'_> {
        self.node(self.root)
    }

    /// Look up a node by id.
    ///
    /// # Panics
    /// If `id` does not belong to this AST.
    pub fn node(&self, id: NodeId) -> NodeRef<'_> {
        assert!(
            id.index() < self.nodes.len(),
            "NodeId does not belong to this AST"
        );
        NodeRef { ast: self, id }
    }

    /// The token stream, including the final EOF token.
    pub fn tokens(&self) -> impl ExactSizeIterator<Item = TokenRef<'_>> + '_ {
        self.tokens.iter().map(|token| TokenRef {
            source: &self.source,
            token,
        })
    }

    pub(crate) fn data(&self, id: NodeId) -> &NodeData {
        &self.nodes[id.index()]
    }

    pub(crate) fn text(&self, span: Span) -> &str {
        &self.source[span.start..span.end]
    }

    /// The AST as a JSON document: source, node tree, and tokens (without
    /// EOF). Node ids are tree positions (`root`, `root.0`, ...); spans have
    /// byte offsets and 1-based lines and byte columns.
    pub fn json(&self) -> JsonAst {
        json::build(self)
    }
}

/// Compact canonical expression (comments dropped).
impl fmt::Display for Ast {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&crate::print::canonical(self, self.root))
    }
}

/// Read-only view of one AST node. Its [`Display`](fmt::Display) output is
/// the node's compact canonical expression.
#[derive(Clone, Copy)]
pub struct NodeRef<'a> {
    ast: &'a Ast,
    id: NodeId,
}

impl fmt::Debug for NodeRef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NodeRef")
            .field("id", &self.id)
            .field("kind", &self.kind())
            .field("text", &self.to_string())
            .finish()
    }
}

impl<'a> NodeRef<'a> {
    /// The node's id, for [`Ast::node`] and [`Edit`](crate::Edit).
    pub fn id(&self) -> NodeId {
        self.id
    }

    fn data(&self) -> &'a NodeData {
        self.ast.data(self.id)
    }

    /// The node's shape.
    pub fn kind(&self) -> AstKind {
        match self.data() {
            NodeData::Literal { .. } => AstKind::Literal,
            NodeData::Path { .. } => AstKind::Path,
            NodeData::Array { .. } => AstKind::Array,
            NodeData::Call { .. } => AstKind::Call,
            NodeData::Unary { .. } => AstKind::Unary,
            NodeData::Binary { .. } => AstKind::Binary,
        }
    }

    /// The node's byte range in the source.
    pub fn span(&self) -> Span {
        self.data().span()
    }

    /// The child nodes in source order: array items, call arguments, the
    /// operand of `not`, or the two operands of a binary operator.
    pub fn children(&self) -> Vec<NodeRef<'a>> {
        let ids: &[NodeId] = match self.data() {
            NodeData::Literal { .. } | NodeData::Path { .. } => &[],
            NodeData::Array { items, .. } => items,
            NodeData::Call { args, .. } => args,
            NodeData::Unary { operand, .. } => std::slice::from_ref(operand),
            NodeData::Binary { lhs, rhs, .. } => {
                return vec![self.ast.node(*lhs), self.ast.node(*rhs)];
            }
        };
        ids.iter().map(|&id| self.ast.node(id)).collect()
    }

    /// Normalized operator for unary and binary nodes. For `not contains`,
    /// `not matches`, and `not in` this is the operator being negated.
    pub fn operator(&self) -> Option<Operator> {
        match self.data() {
            NodeData::Unary { op, .. } | NodeData::Binary { op, .. } => Some(*op),
            _ => None,
        }
    }

    /// Source spelling of the operator for unary and binary nodes.
    pub fn raw_operator(&self) -> Option<&'a str> {
        match self.data() {
            NodeData::Unary { raw_op, .. } | NodeData::Binary { raw_op, .. } => {
                Some(self.ast.text(*raw_op))
            }
            _ => None,
        }
    }

    /// Whether a binary node is `not contains`, `not matches`, or `not in`.
    pub fn negated(&self) -> bool {
        matches!(self.data(), NodeData::Binary { negated: true, .. })
    }

    /// The literal token as written, for literal nodes.
    pub fn literal(&self) -> Option<(LiteralKind, &'a str)> {
        match self.data() {
            NodeData::Literal { span, kind } => Some((*kind, self.ast.text(*span))),
            _ => None,
        }
    }

    /// Path segments, for path nodes.
    pub fn path(&self) -> Option<&'a [Segment]> {
        match self.data() {
            NodeData::Path { segments, .. } => Some(segments),
            _ => None,
        }
    }

    /// Function or macro name, for call nodes.
    pub fn call_name(&self) -> Option<&'a str> {
        match self.data() {
            NodeData::Call { name, .. } => Some(self.ast.text(*name)),
            _ => None,
        }
    }
}

/// Compact canonical expression for the node.
impl fmt::Display for NodeRef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&crate::print::canonical(self.ast, self.id))
    }
}

/// Builder used by the parser; keeps the arena private to the crate.
pub(crate) struct AstBuilder {
    pub nodes: Vec<NodeData>,
}

impl AstBuilder {
    pub fn push(&mut self, node: NodeData) -> NodeId {
        let id = NodeId(u32::try_from(self.nodes.len()).expect("AST node count exceeds u32"));
        self.nodes.push(node);
        id
    }

    pub fn span(&self, id: NodeId) -> Span {
        self.nodes[id.index()].span()
    }
}
