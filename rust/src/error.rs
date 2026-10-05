//! Error types.

use std::fmt;

/// A syntax or literal error in an expression, with a 1-based position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    line: usize,
    column: usize,
    message: String,
    input: String,
    suggestion: Option<&'static str>,
}

impl ParseError {
    /// Build an error at byte offset `pos` of `input` (Go `newParseError`).
    pub(crate) fn at(input: &str, pos: usize, message: impl Into<String>) -> Self {
        let (line, column) = line_column(input, pos);
        let message = message.into();
        let suggestion = suggestion(&message);
        ParseError {
            line,
            column,
            message,
            input: input.to_owned(),
            suggestion,
        }
    }

    /// 1-based line of the error.
    pub fn line(&self) -> usize {
        self.line
    }

    /// 1-based column of the error, counted in bytes.
    pub fn column(&self) -> usize {
        self.column
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn suggestion(&self) -> Option<&str> {
        self.suggestion
    }
}

/// Go `getLineColumn`: walk chars before `pos`; a newline starts a new line,
/// any other char advances the column by its UTF-8 length.
fn line_column(input: &str, pos: usize) -> (usize, usize) {
    let mut line = 1;
    let mut column = 1;
    for (i, c) in input.char_indices() {
        if i >= pos {
            break;
        }
        if c == '\n' {
            line += 1;
            column = 1;
        } else {
            column += c.len_utf8();
        }
    }
    (line, column)
}

fn suggestion(message: &str) -> Option<&'static str> {
    let rules: [(&str, &str); 9] = [
        (
            "parsing string",
            "string values must be properly quoted with matching quotes (e.g. \"hello\")",
        ),
        (
            "parsing integer",
            "integer values must be valid integers without decimals (e.g. 42)",
        ),
        (
            "parsing float",
            "floating-point numbers must be in the format 1.23",
        ),
        (
            "parsing boolean",
            "boolean values must be either 'true' or 'false' (case insensitive)",
        ),
        (
            "parsing IP",
            "IP addresses must be in valid IPv4 (e.g. 192.168.1.1) or IPv6 format",
        ),
        (
            "parsing CIDR",
            "CIDR blocks must be in valid format (e.g. 192.168.1.0/24)",
        ),
        (
            "parsing hex",
            "hex strings must contain valid hex digits optionally separated by colons",
        ),
        (
            "regex",
            "regex patterns must be surrounded by / or | and contain valid regex syntax",
        ),
        (
            "field",
            "field names must be valid identifiers (e.g. 'field_name' or 'field.name')",
        ),
    ];
    rules
        .iter()
        .find(|(needle, _)| message.contains(needle))
        .map(|(_, text)| *text)
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let error_line = self.input.split('\n').nth(self.line - 1).unwrap_or("");
        write!(
            f,
            "syntax error at line {}:{}:\n{error_line}",
            self.line, self.column
        )?;
        if !error_line.is_empty() {
            write!(f, "\n{}^", " ".repeat(self.column - 1))?;
        }
        if !self.message.is_empty() {
            write!(f, "\n{}", self.message)?;
        }
        if let Some(suggestion) = self.suggestion {
            write!(f, "\nsuggestion: {suggestion}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ParseError {}
