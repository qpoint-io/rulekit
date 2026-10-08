//! Macros and the validated evaluation environment.

use std::fmt;

use crate::error::{Error, ParseError};
use crate::func::Function;
use crate::value::Map;
use crate::{Rule, stdlib_arity};

/// A named, zero-argument rule, expanded where it is called as `name()`.
///
/// A macro is evaluated against the same input, context, and [`Env`] as the
/// rule calling it. Calling it with arguments is an [`Error::MacroArgs`].
/// Register macros with [`EnvBuilder::macro_source`] or
/// [`EnvBuilder::macro_rule`].
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

/// Validated custom functions and macros, passed to evaluation through
/// [`Opts`](crate::Opts).
///
/// Build one with [`Env::builder`]. `C` is the evaluation context type the
/// functions receive.
///
/// ```rust
/// use rulekit::value::{Map, Value};
/// use rulekit::{Env, KvInput, Opts};
///
/// let env: Env = Env::builder()
///     .macro_source("is_internal", r"ip in 10.0.0.0/8 or domain matches /\.internal$/")?
///     .build()?;
///
/// let rule = rulekit::parse(r#"is_internal() and user != "root""#)?;
/// let input = KvInput::from_values(Map::from_iter([
///     ("domain".to_owned(), Value::String("api.internal".into())),
///     ("user".to_owned(), Value::String("alice".into())),
/// ]));
/// assert!(rule.eval(&(), &input, Opts::new(&env)).pass());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub struct Env<C: ?Sized = ()> {
    pub(crate) functions: Map<Function<C>>,
    pub(crate) macros: Map<Macro>,
}

impl<C: ?Sized> fmt::Debug for Env<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Env")
            .field("functions", &self.functions)
            .field("macros", &self.macros)
            .finish()
    }
}

impl<C: ?Sized> Default for Env<C> {
    fn default() -> Self {
        Env {
            functions: Map::default(),
            macros: Map::default(),
        }
    }
}

impl<C: ?Sized> Env<C> {
    /// An environment with no custom functions or macros.
    pub fn new() -> Self {
        Self::default()
    }

    /// Start building an environment.
    pub fn builder() -> EnvBuilder<C> {
        EnvBuilder {
            env: Env::default(),
            duplicates: Vec::new(),
        }
    }

    /// The custom function named `name`.
    pub fn function(&self, name: &str) -> Option<&Function<C>> {
        self.functions.get(name)
    }

    /// The macro named `name`.
    pub fn macro_rule(&self, name: &str) -> Option<&Macro> {
        self.macros.get(name)
    }
}

/// Builds an [`Env`]; names are checked once in [`EnvBuilder::build`].
pub struct EnvBuilder<C: ?Sized = ()> {
    env: Env<C>,
    duplicates: Vec<String>,
}

impl<C: ?Sized> EnvBuilder<C> {
    /// Add a custom function under its schema name.
    pub fn function(mut self, function: Function<C>) -> Self {
        let name = function.name().to_owned();
        if self.env.functions.insert(name.clone(), function).is_some() {
            self.duplicates.push(name);
        }
        self
    }

    /// Add (or replace) a macro.
    pub fn macro_rule(mut self, name: impl Into<String>, macro_: Macro) -> Self {
        self.env.macros.insert(name.into(), macro_);
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

    /// Validate and return the environment.
    ///
    /// # Errors
    ///
    /// [`Error::Env`] if two functions have the same name, a function or
    /// macro has the name of a standard library function (such as
    /// `starts_with`), or a macro has the name of a custom function.
    pub fn build(self) -> Result<Env<C>, Error> {
        if let Some(name) = self.duplicates.first() {
            return Err(Error::Env(format!(
                "function {name:?}: defined more than once"
            )));
        }
        let mut names: Vec<&String> = self.env.functions.keys().collect();
        names.sort();
        for name in names {
            if stdlib_arity(name).is_some() {
                return Err(Error::Env(format!(
                    "function {name:?}: name conflicts with a stdlib function"
                )));
            }
        }
        let mut names: Vec<&String> = self.env.macros.keys().collect();
        names.sort();
        for name in names {
            if stdlib_arity(name).is_some() {
                return Err(Error::Env(format!(
                    "macro {name:?}: name conflicts with a stdlib function"
                )));
            }
            if self.env.functions.contains_key(name) {
                return Err(Error::Env(format!(
                    "macro {name:?}: name conflicts with a custom function"
                )));
            }
        }
        Ok(self.env)
    }
}
