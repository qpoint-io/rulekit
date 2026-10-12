//! Evaluation options and macros.

use crate::error::{Error, ParseError};
use crate::func::Function;
use crate::value::Map;
use crate::{Rule, stdlib_arity};

/// A named, zero-argument rule, expanded where it is called as `name()`.
///
/// A macro is evaluated against the same input and [`Opts`] as the rule
/// calling it. Calling it with arguments is an [`Error::MacroArgs`].
/// Register macros with [`OptsBuilder::macro_source`] or
/// [`OptsBuilder::macro_rule`].
#[derive(Clone, Debug)]
pub struct Macro {
    source: String,
    doc: Option<String>,
    pub(crate) rule: Rule,
}

impl Macro {
    /// Parse a macro body.
    ///
    /// # Errors
    ///
    /// A [`ParseError`] if `source` is not a valid expression.
    pub fn new(source: &str) -> Result<Macro, ParseError> {
        Ok(Macro {
            source: source.to_owned(),
            doc: None,
            rule: crate::parse(source)?,
        })
    }

    /// The same macro with documentation, for tools that list macros.
    pub fn with_doc(mut self, doc: impl Into<String>) -> Self {
        self.doc = Some(doc.into());
        self
    }

    /// The macro's documentation, if set.
    pub fn doc(&self) -> Option<&str> {
        self.doc.as_deref()
    }

    /// The macro body as written.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The compiled macro body.
    pub fn rule(&self) -> &Rule {
        &self.rule
    }
}

/// Options for [`Rule::eval`]: custom [`Function`]s, [`Macro`]s, and whether
/// to record a [`Trace`](crate::Trace).
///
/// Names are validated once, by [`OptsBuilder::build`], so evaluation never
/// validates. `Opts::default()` has no functions or macros and tracing off,
/// for rules that call neither. An `Opts` is immutable and can be shared
/// across threads; keep one for as long as you evaluate rules with it, since
/// an [`EvalResult`](crate::EvalResult) borrows from it.
///
/// ```rust
/// use rulekit::value::{Map, Value};
/// use rulekit::{KvInput, Opts};
///
/// let opts = Opts::builder()
///     .macro_source("is_internal", r"ip in 10.0.0.0/8 or domain matches /\.internal$/")?
///     .build()?;
///
/// let rule = rulekit::parse(r#"is_internal() and user != "root""#)?;
/// let input = KvInput::from_values(Map::from_iter([
///     ("domain".to_owned(), Value::String("api.internal".into())),
///     ("user".to_owned(), Value::String("alice".into())),
/// ]));
/// assert!(rule.eval(&input, &opts).pass());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone, Debug, Default)]
pub struct Opts {
    functions: Map<Function>,
    macros: Map<Macro>,
    trace: bool,
}

impl Opts {
    /// Start building options.
    pub fn builder() -> OptsBuilder {
        OptsBuilder::default()
    }

    /// Whether evaluation records a [`Trace`](crate::Trace). Untraced
    /// evaluation runs code with no trace bookkeeping at all.
    pub fn trace(&self) -> bool {
        self.trace
    }

    /// The custom function named `name`.
    pub fn function(&self, name: &str) -> Option<&Function> {
        self.functions.get(name)
    }

    /// The macro named `name`.
    pub fn macro_rule(&self, name: &str) -> Option<&Macro> {
        self.macros.get(name)
    }
}

/// Builds [`Opts`]; names are checked once in [`OptsBuilder::build`].
#[derive(Default)]
pub struct OptsBuilder {
    opts: Opts,
    duplicates: Vec<String>,
}

impl OptsBuilder {
    /// Add a custom function under its schema name.
    pub fn function(mut self, function: Function) -> Self {
        let name = function.name().to_owned();
        if self.opts.functions.insert(name.clone(), function).is_some() {
            self.duplicates.push(name);
        }
        self
    }

    /// Add (or replace) a macro.
    pub fn macro_rule(mut self, name: impl Into<String>, macro_: Macro) -> Self {
        self.opts.macros.insert(name.into(), macro_);
        self
    }

    /// Parse `source` and add (or replace) it as a macro.
    ///
    /// # Errors
    ///
    /// A [`ParseError`] if `source` is not a valid expression.
    pub fn macro_source(self, name: impl Into<String>, source: &str) -> Result<Self, ParseError> {
        Ok(self.macro_rule(name, Macro::new(source)?))
    }

    /// Record an evaluation [`Trace`](crate::Trace) (off by default).
    pub fn trace(mut self, trace: bool) -> Self {
        self.opts.trace = trace;
        self
    }

    /// Validate and return the options.
    ///
    /// # Errors
    ///
    /// [`Error::Opts`] if two functions have the same name, a function or
    /// macro has the name of a standard library function (such as
    /// `starts_with`), or a macro has the name of a custom function.
    pub fn build(self) -> Result<Opts, Error> {
        if let Some(name) = self.duplicates.first() {
            return Err(Error::Opts(format!(
                "function {name:?}: defined more than once"
            )));
        }
        let mut names: Vec<&String> = self.opts.functions.keys().collect();
        names.sort();
        for name in names {
            if stdlib_arity(name).is_some() {
                return Err(Error::Opts(format!(
                    "function {name:?}: name conflicts with a stdlib function"
                )));
            }
        }
        let mut names: Vec<&String> = self.opts.macros.keys().collect();
        names.sort();
        for name in names {
            if stdlib_arity(name).is_some() {
                return Err(Error::Opts(format!(
                    "macro {name:?}: name conflicts with a stdlib function"
                )));
            }
            if self.opts.functions.contains_key(name) {
                return Err(Error::Opts(format!(
                    "macro {name:?}: name conflicts with a custom function"
                )));
            }
        }
        Ok(self.opts)
    }
}
