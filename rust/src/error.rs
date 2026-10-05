//! Error types.

use std::fmt;

/// A syntax or literal error in an expression, with a 1-based position.
///
/// The [`Display`](fmt::Display) form names the position, shows the
/// offending source line with a caret under the column, then the message and
/// any [`suggestion`](Self::suggestion).
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

    /// The error message, without position or suggestion.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// A hint on the expected syntax, for some kinds of error.
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

/// A boxed error from a caller-provided input or function.
pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// An evaluation, environment, or input-decoding error.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// A call to a name that is not a function or macro.
    UnknownFunction(String),
    /// A function called with the wrong number of arguments.
    ArgCount {
        /// The function name.
        function: String,
        /// The number of declared parameters (before the rest parameter).
        expected: usize,
        /// The number of arguments passed.
        got: usize,
        /// Whether the function takes more arguments (a rest parameter), so
        /// `expected` is a minimum.
        variadic: bool,
    },
    /// A macro called with arguments.
    MacroArgs {
        /// The macro name.
        name: String,
        /// The number of arguments passed.
        got: usize,
    },
    /// A function asked for an argument name it does not declare.
    UnknownArg(String),
    /// A function argument of the wrong type.
    InvalidArg {
        /// The parameter name (or its index, if the function declares no
        /// name for it).
        name: String,
        /// The expected type name.
        expected: String,
        /// The type name of the value passed (`nothing` if absent).
        got: String,
    },
    /// An input failed to resolve a field.
    Input {
        /// The field path in canonical form, as in missing fields.
        field: String,
        /// The input's error.
        source: BoxError,
    },
    /// A function returned an error.
    Function {
        /// The function name.
        name: String,
        /// The function's error.
        source: BoxError,
    },
    /// Several errors (both sides of `and`/`or` failed).
    Multiple(Vec<Error>),
    /// Invalid functions or macros when building an `Env`.
    Env(String),
    /// Invalid JSON input for `decode_json`.
    Json(String),
    /// Invalid edits for `rewrite`.
    Rewrite(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::UnknownFunction(name) => write!(f, "unknown function {name:?}"),
            Error::ArgCount {
                function,
                expected,
                got,
                variadic,
            } => {
                let at_least = if *variadic { "at least " } else { "" };
                write!(
                    f,
                    "function {function:?} expects {at_least}{expected} arguments, got {got}"
                )
            }
            Error::MacroArgs { name, got } => {
                write!(f, "macro {name:?} expects 0 arguments, got {got}")
            }
            Error::UnknownArg(name) => write!(f, "unrecognized argument name {name:?}"),
            Error::InvalidArg {
                name,
                expected,
                got,
            } => write!(f, "arg {name}: expected {expected}, got {got}"),
            Error::Input { field, source } => write!(f, "field {field:?}: {source}"),
            Error::Function { name, source } => write!(f, "function {name:?}: {source}"),
            Error::Multiple(errors) => {
                write!(f, "{} errors occurred:", errors.len())?;
                for err in errors {
                    write!(f, "\n\t* {err}")?;
                }
                Ok(())
            }
            Error::Env(msg) | Error::Json(msg) | Error::Rewrite(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Input { source, .. } | Error::Function { source, .. } => Some(source.as_ref()),
            _ => None,
        }
    }
}
