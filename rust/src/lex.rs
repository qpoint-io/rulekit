//! Lexer (port of the lexer half of `parser.go`).

use crate::ast::{Span, Token, TokenKind};
use crate::literal;
use crate::value::{Cidr, Ip};

/// A lexing failure: the byte offset the error is reported at, and a message.
#[derive(Debug)]
pub(crate) struct LexError {
    pub pos: usize,
    pub message: String,
}

/// Lex the whole input. The returned tokens end with EOF and carry leading and
/// trailing trivia.
pub(crate) fn lex(input: &str) -> Result<Vec<Token>, LexError> {
    let mut lexer = Lexer {
        input,
        bytes: input.as_bytes(),
        pos: 0,
    };
    let mut tokens: Vec<Token> = Vec::new();
    loop {
        let token = lexer.next()?;
        tokens.push(token);
        if token.kind == TokenKind::Eof {
            break;
        }
    }
    for i in 0..tokens.len() - 1 {
        tokens[i].trailing = tokens[i + 1].leading;
    }
    let last = tokens.len() - 1;
    tokens[last].trailing = Span::new(input.len(), input.len());
    Ok(tokens)
}

struct Lexer<'a> {
    input: &'a str,
    bytes: &'a [u8],
    pos: usize,
}

impl Lexer<'_> {
    fn token(&self, kind: TokenKind, start: usize, leading: Span) -> Token {
        Token {
            kind,
            span: Span::new(start, self.pos),
            leading,
            trailing: Span::default(),
        }
    }

    fn error<T>(&self, pos: usize, message: impl Into<String>) -> Result<T, LexError> {
        Err(LexError {
            pos,
            message: message.into(),
        })
    }

    fn next(&mut self) -> Result<Token, LexError> {
        let leading = self.skip_ignored()?;
        if self.pos >= self.bytes.len() {
            return Ok(self.token(TokenKind::Eof, self.pos, leading));
        }

        let start = self.pos;
        let ch = self.bytes[start];
        let single = |lexer: &mut Self, kind| {
            lexer.pos += 1;
            Ok(lexer.token(kind, start, leading))
        };
        match ch {
            b'(' => return single(self, TokenKind::LParen),
            b')' => return single(self, TokenKind::RParen),
            b'[' => return single(self, TokenKind::LBracket),
            b']' => return single(self, TokenKind::RBracket),
            b',' => return single(self, TokenKind::Comma),
            b'.' => return single(self, TokenKind::Dot),
            b'!' => {
                if self.eat("!=") {
                    return Ok(self.token(TokenKind::Ne, start, leading));
                }
                return single(self, TokenKind::Not);
            }
            b'&' => {
                if self.eat("&&") {
                    return Ok(self.token(TokenKind::And, start, leading));
                }
            }
            b'|' => {
                if self.eat("||") {
                    return Ok(self.token(TokenKind::Or, start, leading));
                }
                return self.scan_regex(b'|', leading);
            }
            b'=' => {
                if self.eat("==") {
                    return Ok(self.token(TokenKind::Eq, start, leading));
                }
                if self.eat("=~") {
                    return Ok(self.token(TokenKind::Matches, start, leading));
                }
            }
            b'<' => {
                if self.eat("<=") {
                    return Ok(self.token(TokenKind::Le, start, leading));
                }
                return single(self, TokenKind::Lt);
            }
            b'>' => {
                if self.eat(">=") {
                    return Ok(self.token(TokenKind::Ge, start, leading));
                }
                return single(self, TokenKind::Gt);
            }
            b'x' | b'X' => {
                if let Some(&quote @ (b'"' | b'\'')) = self.bytes.get(start + 1) {
                    self.pos += 1;
                    let mut token =
                        self.scan_delimited(quote, TokenKind::HexString, leading, start)?;
                    token.span.start = start;
                    return Ok(token);
                }
            }
            b'\'' | b'"' | b'`' => {
                return self.scan_delimited(ch, TokenKind::String, leading, start);
            }
            b'/' => return self.scan_regex(b'/', leading),
            _ => {}
        }

        if matches!(ch, b'+' | b'-' | b':') || is_atom_start(ch) || ch.is_ascii_digit() {
            return self.scan_atom(leading);
        }

        self.error(
            start,
            format!("unexpected character: {:?}", char_at(self.input, start)),
        )
    }

    /// Skip whitespace and comments; returns the skipped span.
    fn skip_ignored(&mut self) -> Result<Span, LexError> {
        let start = self.pos;
        while self.pos < self.bytes.len() {
            let c = self.input[self.pos..]
                .chars()
                .next()
                .expect("pos is on a char boundary");
            if c.is_whitespace() {
                self.pos += c.len_utf8();
                continue;
            }
            if self.has_prefix("--") {
                self.pos += 2;
                while self.pos < self.bytes.len() && self.bytes[self.pos] != b'\n' {
                    self.pos += 1;
                }
                continue;
            }
            if self.has_prefix("/*") {
                match self.input[self.pos + 2..].find("*/") {
                    Some(end) => {
                        self.pos += end + 4;
                        continue;
                    }
                    None => return self.error(self.pos, "unterminated block comment"),
                }
            }
            break;
        }
        Ok(Span::new(start, self.pos))
    }

    /// Scan a literal delimited by `delim` starting at `self.pos`, honouring
    /// backslash escapes except in backticks. `error_pos` locates unterminated literals.
    fn scan_delimited(
        &mut self,
        delim: u8,
        kind: TokenKind,
        leading: Span,
        error_pos: usize,
    ) -> Result<Token, LexError> {
        let start = self.pos;
        self.pos += 1;
        let mut escaped = false;
        while self.pos < self.bytes.len() {
            let ch = self.bytes[self.pos];
            self.pos += 1;
            if escaped {
                escaped = false;
                continue;
            }
            if ch == b'\\' && delim != b'`' {
                escaped = true;
                continue;
            }
            if ch == delim {
                return Ok(self.token(kind, start, leading));
            }
        }
        self.error(error_pos, "unterminated literal")
    }

    /// Scan a delimited regex plus a directly following run of `i`, `m`, `s`
    /// flags. A letter run containing anything else is left for the next token,
    /// so `/x/and` lexes as a regex followed by `and`.
    fn scan_regex(&mut self, delim: u8, leading: Span) -> Result<Token, LexError> {
        let start = self.pos;
        let mut token = self.scan_delimited(delim, TokenKind::Regex, leading, start)?;
        let mut end = self.pos;
        while end < self.bytes.len() && self.bytes[end].is_ascii_alphabetic() {
            end += 1;
        }
        let flags = &self.bytes[self.pos..end];
        if !flags.is_empty() && flags.iter().all(|b| matches!(b, b'i' | b'm' | b's')) {
            self.pos = end;
            token.span.end = end;
        }
        Ok(token)
    }

    fn scan_atom(&mut self, leading: Span) -> Result<Token, LexError> {
        let start = self.pos;
        while self.pos < self.bytes.len() {
            let ch = self.bytes[self.pos];
            // Atoms are ASCII; a non-ASCII byte ends the atom.
            if !ch.is_ascii() || is_go_ascii_space(ch) || b"()[],<>=!&|\"'".contains(&ch) {
                break;
            }
            self.pos += 1;
        }
        let raw = &self.input[start..self.pos];
        let keyword = match raw.to_ascii_lowercase().as_str() {
            "not" => Some(TokenKind::Not),
            "and" => Some(TokenKind::And),
            "or" => Some(TokenKind::Or),
            "eq" => Some(TokenKind::Eq),
            "ne" => Some(TokenKind::Ne),
            "gt" => Some(TokenKind::Gt),
            "ge" => Some(TokenKind::Ge),
            "lt" => Some(TokenKind::Lt),
            "le" => Some(TokenKind::Le),
            "contains" => Some(TokenKind::Contains),
            "matches" => Some(TokenKind::Matches),
            "in" => Some(TokenKind::In),
            "true" | "false" => Some(TokenKind::Bool),
            _ => None,
        };
        let kind = if let Some(kind) = keyword {
            kind
        } else if Cidr::parse(raw).is_ok() {
            TokenKind::IpCidr
        } else if Ip::parse(raw).is_ok() {
            TokenKind::Ip
        } else if literal::parse_int(raw).is_some() {
            TokenKind::Int
        } else if literal::is_float(raw) {
            TokenKind::Float
        } else if is_hex_string(raw) {
            TokenKind::HexString
        } else if is_field(raw) {
            TokenKind::Field
        } else {
            return self.error(start, format!("unexpected token: {raw:?}"));
        };
        Ok(self.token(kind, start, leading))
    }

    fn eat(&mut self, s: &str) -> bool {
        if !self.has_prefix(s) {
            return false;
        }
        self.pos += s.len();
        true
    }

    fn has_prefix(&self, s: &str) -> bool {
        self.bytes[self.pos..].starts_with(s.as_bytes())
    }
}

/// The character starting at byte `pos`, or the raw byte if `pos` is not a
/// char boundary (only used in messages).
fn char_at(input: &str, pos: usize) -> String {
    match input.get(pos..).and_then(|s| s.chars().next()) {
        Some(c) => c.to_string(),
        None => format!("\\x{:02x}", input.as_bytes()[pos]),
    }
}

/// Go `unicode.IsSpace` restricted to ASCII.
fn is_go_ascii_space(b: u8) -> bool {
    matches!(b, b'\t' | b'\n' | 0x0b | 0x0c | b'\r' | b' ')
}

/// Field names: an ASCII letter or `_`, then letters, digits, `_`, `.`, or `-`.
fn is_atom_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_field(s: &str) -> bool {
    let bytes = s.as_bytes();
    !bytes.is_empty()
        && is_atom_start(bytes[0])
        && bytes[1..]
            .iter()
            .all(|&c| is_atom_start(c) || c.is_ascii_digit() || c == b'.' || c == b'-')
}

/// Colon-separated hex pairs (at least one colon).
fn is_hex_string(s: &str) -> bool {
    s.contains(':')
        && s.split(':')
            .all(|part| part.len() == 2 && part.bytes().all(|b| b.is_ascii_hexdigit()))
}
