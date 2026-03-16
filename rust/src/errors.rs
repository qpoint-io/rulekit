use std::collections::HashSet;
use std::fmt;

#[derive(Debug, Clone)]
pub enum EvalError {
    MissingFields(HashSet<String>),
    InvalidOperation(String),
    UnknownFunction(String),
    Multiple(Vec<EvalError>),
}

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EvalError::MissingFields(fields) => {
                let mut sorted: Vec<_> = fields.iter().collect();
                sorted.sort();
                write!(f, "missing fields: {}", sorted.into_iter().cloned().collect::<Vec<_>>().join(", "))
            }
            EvalError::InvalidOperation(msg) => write!(f, "invalid operation: {}", msg),
            EvalError::UnknownFunction(name) => write!(f, "unknown function: {}", name),
            EvalError::Multiple(errs) => {
                for (i, e) in errs.iter().enumerate() {
                    if i > 0 {
                        write!(f, "; ")?;
                    }
                    write!(f, "{}", e)?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for EvalError {}

/// Merge multiple errors, coalescing MissingFields sets.
pub fn coalesce_errors(errs: Vec<Option<EvalError>>) -> Option<EvalError> {
    let mut missing = HashSet::new();
    let mut others = Vec::new();

    for err in errs.into_iter().flatten() {
        match err {
            EvalError::MissingFields(fields) => {
                missing.extend(fields);
            }
            other => others.push(other),
        }
    }

    if !missing.is_empty() {
        others.push(EvalError::MissingFields(missing));
    }

    match others.len() {
        0 => None,
        1 => Some(others.into_iter().next().unwrap()),
        _ => Some(EvalError::Multiple(others)),
    }
}
