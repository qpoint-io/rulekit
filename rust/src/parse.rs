//! Pratt parser (port of the parser half of `parser.go`).

use crate::ast::{
    Ast, AstBuilder, LiteralKind, NodeData, NodeId, Operator, Segment, Span, Token, TokenKind,
};
use crate::error::ParseError;
use crate::lex::lex;
use crate::literal;

/// Precedence of comparison operators; `not` parses its operand above it.
const COMPARISON_PRECEDENCE: u8 = 3;

pub(crate) fn parse(source: &str) -> Result<Ast, ParseError> {
    let tokens = lex(source).map_err(|err| ParseError::at(source, err.pos, err.message))?;
    let mut parser = Parser {
        input: source,
        tokens: &tokens,
        pos: 0,
        ast: AstBuilder { nodes: Vec::new() },
    };
    let root = parser.parse_expr(0)?;
    let tok = parser.peek();
    if tok.kind != TokenKind::Eof {
        return Err(parser.error(tok, format!("unexpected token {:?}", parser.raw(tok))));
    }
    let nodes = parser.ast.nodes;
    Ok(Ast {
        source: source.to_owned(),
        nodes,
        root,
        tokens,
    })
}

struct Parser<'a> {
    input: &'a str,
    tokens: &'a [Token],
    pos: usize,
    ast: AstBuilder,
}

type PResult<T> = Result<T, ParseError>;

/// Go `infixPrecedence`; `None` for tokens that are not infix operators.
fn infix_precedence(kind: TokenKind) -> Option<u8> {
    match kind {
        TokenKind::Or => Some(1),
        TokenKind::And => Some(2),
        TokenKind::Eq
        | TokenKind::Ne
        | TokenKind::Contains
        | TokenKind::Gt
        | TokenKind::Ge
        | TokenKind::Lt
        | TokenKind::Le
        | TokenKind::Matches
        | TokenKind::In => Some(COMPARISON_PRECEDENCE),
        _ => None,
    }
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Token {
        self.tokens[self.pos]
    }

    /// The token `n` positions ahead, or the final EOF token.
    fn peek_at(&self, n: usize) -> Token {
        self.tokens
            .get(self.pos + n)
            .copied()
            .unwrap_or(self.tokens[self.tokens.len() - 1])
    }

    fn next(&mut self) -> Token {
        let tok = self.tokens[self.pos];
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        tok
    }

    fn prev_end(&self) -> usize {
        self.tokens[self.pos - 1].span.end
    }

    fn raw(&self, tok: Token) -> &'a str {
        &self.input[tok.span.start..tok.span.end]
    }

    fn error(&self, tok: Token, message: impl Into<String>) -> ParseError {
        ParseError::at(self.input, tok.span.start, message)
    }

    fn expect(&mut self, kind: TokenKind) -> PResult<Token> {
        let tok = self.next();
        if tok.kind != kind {
            return Err(self.error(tok, format!("expected {}", expected_name(kind))));
        }
        Ok(tok)
    }

    fn span(&self, id: NodeId) -> Span {
        self.ast.span(id)
    }

    fn is_path(&self, id: NodeId) -> bool {
        matches!(self.ast.nodes[id.index()], NodeData::Path { .. })
    }

    fn parse_expr(&mut self, min_prec: u8) -> PResult<NodeId> {
        let left = self.parse_primary()?;
        let mut left = self.parse_postfix(left)?;

        loop {
            let tok = self.peek();
            let mut kind = tok.kind;
            let mut negated = false;
            // `not` directly before contains, matches, or in negates that operator.
            if tok.kind == TokenKind::Not && self.raw(tok) != "!" {
                let next = self.peek_at(1);
                if matches!(
                    next.kind,
                    TokenKind::Contains | TokenKind::Matches | TokenKind::In
                ) {
                    kind = next.kind;
                    negated = true;
                }
            }
            let Some(prec) = infix_precedence(kind) else {
                break;
            };
            if prec < min_prec {
                break;
            }
            self.next();
            let mut raw_op = tok.span;
            if negated {
                let op_tok = self.next();
                raw_op = Span::new(tok.span.start, op_tok.span.end);
            }
            let op = kind.operator().expect("infix token is an operator");

            let right = match kind {
                TokenKind::And | TokenKind::Or => self.parse_expr(prec + 1)?,
                TokenKind::Matches => {
                    let rhs = self.peek();
                    if rhs.kind != TokenKind::Regex {
                        return Err(self.error(rhs, "matches requires a regex value"));
                    }
                    let right = self.parse_primary()?;
                    self.parse_postfix(right)?
                }
                TokenKind::In => {
                    let right = self.parse_expr(prec + 1)?;
                    let is_cidr = matches!(
                        self.ast.nodes[right.index()],
                        NodeData::Literal {
                            kind: LiteralKind::Cidr,
                            ..
                        }
                    );
                    let is_array = matches!(self.ast.nodes[right.index()], NodeData::Array { .. });
                    // A path is a list (or a CIDR) known only at evaluation.
                    // Literals other than an array or CIDR stay a parse error,
                    // matching `a not in 1` in the shared vectors.
                    let is_path = matches!(self.ast.nodes[right.index()], NodeData::Path { .. });
                    if !is_cidr && !is_array && !is_path {
                        return Err(self.error(tok, "in requires an array or CIDR value"));
                    }
                    right
                }
                _ => {
                    let right = self.parse_expr(prec + 1)?;
                    let inequality = matches!(
                        op,
                        Operator::Gt | Operator::Ge | Operator::Lt | Operator::Le
                    );
                    if inequality
                        && (!self.valid_inequality_operand(left)
                            || !self.valid_inequality_operand(right))
                    {
                        return Err(self.error(tok, "invalid operation"));
                    }
                    right
                }
            };
            let span = self.span(left).join(self.span(right));
            left = self.ast.push(NodeData::Binary {
                span,
                op,
                raw_op,
                negated,
                lhs: left,
                rhs: right,
            });
        }
        Ok(left)
    }

    /// Go `astValidInequalityOperand`: paths, calls, and number literals.
    fn valid_inequality_operand(&self, id: NodeId) -> bool {
        match &self.ast.nodes[id.index()] {
            NodeData::Path { .. } | NodeData::Call { .. } => true,
            NodeData::Literal { kind, .. } => matches!(kind, LiteralKind::Int | LiteralKind::Float),
            _ => false,
        }
    }

    fn parse_primary(&mut self) -> PResult<NodeId> {
        let tok = self.next();
        match tok.kind {
            TokenKind::Field => {
                if self.peek().kind == TokenKind::LParen {
                    return self.parse_function(tok);
                }
                let segments = field_segments(self.raw(tok)).collect();
                Ok(self.ast.push(NodeData::Path {
                    span: tok.span,
                    segments,
                }))
            }
            TokenKind::String
            | TokenKind::Int
            | TokenKind::Float
            | TokenKind::Bool
            | TokenKind::Ip
            | TokenKind::IpCidr
            | TokenKind::HexString
            | TokenKind::Regex => {
                let kind = LiteralKind::from_token(tok.kind).expect("literal token");
                Ok(self.ast.push(NodeData::Literal {
                    span: tok.span,
                    kind,
                }))
            }
            TokenKind::LParen => {
                let expr = self.parse_expr(0)?;
                self.expect(TokenKind::RParen)?;
                Ok(expr)
            }
            TokenKind::LBracket => {
                if self.is_root_bracket_path() {
                    let segment = self.parse_bracket_segment()?;
                    let span = Span::new(tok.span.start, self.prev_end());
                    return Ok(self.ast.push(NodeData::Path {
                        span,
                        segments: Box::new([segment]),
                    }));
                }
                self.parse_array(tok)
            }
            TokenKind::Not => {
                // not binds looser than comparisons and tighter than and/or, so
                // `not a == 1` is `not (a == 1)`.
                let operand = self.parse_expr(COMPARISON_PRECEDENCE)?;
                let span = tok.span.join(self.span(operand));
                Ok(self.ast.push(NodeData::Unary {
                    span,
                    op: Operator::Not,
                    raw_op: tok.span,
                    operand,
                }))
            }
            TokenKind::Eof => Err(self.error(tok, "empty expression")),
            _ => Err(self.error(tok, format!("unexpected token {:?}", self.raw(tok)))),
        }
    }

    fn parse_postfix(&mut self, left: NodeId) -> PResult<NodeId> {
        loop {
            match self.peek().kind {
                TokenKind::LBracket => {
                    let start = self.next();
                    let segment = self.parse_bracket_segment()?;
                    if !self.is_path(left) {
                        return Err(self.error(start, "bracket indexing requires a field path"));
                    }
                    let end = self.prev_end();
                    self.extend_path(left, std::iter::once(segment), end);
                }
                TokenKind::Dot => {
                    let dot = self.next();
                    let tok = self.expect(TokenKind::Field)?;
                    if !self.is_path(left) {
                        return Err(self.error(dot, "dot traversal requires a field path"));
                    }
                    let segments: Vec<Segment> = field_segments(self.raw(tok)).collect();
                    self.extend_path(left, segments, tok.span.end);
                }
                _ => return Ok(left),
            }
        }
    }

    /// Append segments to a path node in place. The path is always the most
    /// recently pushed node, so the arena stays in construction order.
    fn extend_path(&mut self, id: NodeId, more: impl IntoIterator<Item = Segment>, end: usize) {
        if let NodeData::Path { span, segments } = &mut self.ast.nodes[id.index()] {
            let mut all = std::mem::take(segments).into_vec();
            all.extend(more);
            *segments = all.into_boxed_slice();
            span.end = end;
        }
    }

    fn is_root_bracket_path(&self) -> bool {
        if self.pos + 2 >= self.tokens.len() {
            return false;
        }
        self.tokens[self.pos].kind == TokenKind::String
            && self.tokens[self.pos + 1].kind == TokenKind::RBracket
            && matches!(
                self.tokens[self.pos + 2].kind,
                TokenKind::LBracket
                    | TokenKind::Dot
                    | TokenKind::Eq
                    | TokenKind::Ne
                    | TokenKind::Gt
                    | TokenKind::Ge
                    | TokenKind::Lt
                    | TokenKind::Le
                    | TokenKind::Contains
                    | TokenKind::Matches
                    | TokenKind::In
            )
    }

    fn parse_bracket_segment(&mut self) -> PResult<Segment> {
        let tok = self.next();
        let segment = match tok.kind {
            TokenKind::String => {
                let key = literal::unquote(self.raw(tok)).map_err(|err| self.error(tok, err))?;
                if key.is_empty() {
                    return Err(self.error(tok, "bracket key must not be empty"));
                }
                Segment::Key { key, bracket: true }
            }
            TokenKind::Int => {
                let raw = self.raw(tok);
                // Go strconv.ParseUint(raw, 10, 0): decimal digits only.
                let index = if raw.bytes().all(|b| b.is_ascii_digit()) {
                    raw.parse::<u64>().ok()
                } else {
                    None
                };
                match index.and_then(|i| usize::try_from(i).ok()) {
                    Some(i) => Segment::Index(i),
                    None => return Err(self.error(tok, "array index must be an unsigned integer")),
                }
            }
            _ => {
                return Err(self.error(
                    tok,
                    "bracket key must be a quoted string or unsigned integer",
                ));
            }
        };
        self.expect(TokenKind::RBracket)?;
        Ok(segment)
    }

    fn parse_array(&mut self, start: Token) -> PResult<NodeId> {
        if self.peek().kind == TokenKind::RBracket {
            return Err(self.error(self.peek(), "array requires at least one value"));
        }
        let mut items = Vec::new();
        loop {
            items.push(self.parse_array_value()?);
            if self.peek().kind != TokenKind::Comma {
                break;
            }
            self.next();
            if self.peek().kind == TokenKind::RBracket {
                return Err(self.error(self.peek(), "trailing commas are not allowed"));
            }
        }
        self.expect(TokenKind::RBracket)?;
        let span = Span::new(start.span.start, self.prev_end());
        Ok(self.ast.push(NodeData::Array {
            span,
            items: items.into_boxed_slice(),
        }))
    }

    fn parse_array_value(&mut self) -> PResult<NodeId> {
        let tok = self.peek();
        match tok.kind {
            TokenKind::LBracket => Err(self.error(tok, "nested arrays are not allowed")),
            TokenKind::Field
            | TokenKind::String
            | TokenKind::Int
            | TokenKind::Float
            | TokenKind::Bool
            | TokenKind::Ip
            | TokenKind::IpCidr
            | TokenKind::HexString
            | TokenKind::Regex => {
                let value = self.parse_primary()?;
                self.parse_postfix(value)
            }
            _ => Err(self.error(tok, "array values must be literals or fields")),
        }
    }

    fn parse_function(&mut self, name: Token) -> PResult<NodeId> {
        self.next(); // (
        let mut args = Vec::new();
        if self.peek().kind != TokenKind::RParen {
            loop {
                args.push(self.parse_expr(0)?);
                if self.peek().kind != TokenKind::Comma {
                    break;
                }
                self.next();
            }
        }
        let end = self.expect(TokenKind::RParen)?;
        let name_text = self.raw(name);
        if let Some(arity) = crate::stdlib_arity(name_text)
            && arity != args.len()
        {
            let message = format!(
                "function {name_text:?} expects {arity} arguments, got {}",
                args.len()
            );
            return Err(ParseError::at(self.input, end.span.end, message));
        }
        let span = Span::new(name.span.start, end.span.end);
        Ok(self.ast.push(NodeData::Call {
            span,
            name: name.span,
            args: args.into_boxed_slice(),
        }))
    }
}

/// Go `fieldPathSegments`: split a field token on `.` (empty parts included).
fn field_segments(raw: &str) -> impl Iterator<Item = Segment> + '_ {
    raw.split('.').map(|part| Segment::Key {
        key: part.to_owned(),
        bracket: false,
    })
}

/// Go `valueTokenString` for the token kinds `expect` is used with.
fn expected_name(kind: TokenKind) -> &'static str {
    match kind {
        TokenKind::Field => "field identifier",
        TokenKind::RParen => "\")\"",
        TokenKind::RBracket => "\"]\"",
        _ => kind.name(),
    }
}
