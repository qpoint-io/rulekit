use std::collections::HashMap;
use std::net::IpAddr;

use ipnet::IpNet;
use regex::Regex;

use crate::ast::{Expr, LiteralValue, Operator};
use crate::compare;
use crate::errors::{coalesce_errors, EvalError};

/// Runtime value produced during evaluation.
#[derive(Debug, Clone)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Ip(IpAddr),
    IpCidr(IpNet),
    HexString(Vec<u8>),
    Regex(Regex),
    Bytes(Vec<u8>),
    Array(Vec<Value>),
    Map(HashMap<String, Value>),
    Null,
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Bool(b) => write!(f, "{}", b),
            Value::Int(n) => write!(f, "{}", n),
            Value::Float(n) => write!(f, "{}", n),
            Value::String(s) => write!(f, "\"{}\"", s),
            Value::Ip(ip) => write!(f, "{}", ip),
            Value::IpCidr(net) => write!(f, "{}", net),
            Value::HexString(bytes) | Value::Bytes(bytes) => {
                let hex: Vec<std::string::String> =
                    bytes.iter().map(|b| format!("{:02x}", b)).collect();
                write!(f, "{}", hex.join(":"))
            }
            Value::Regex(r) => write!(f, "/{}/", r),
            Value::Array(vals) => {
                write!(f, "[")?;
                for (i, v) in vals.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", v)?;
                }
                write!(f, "]")
            }
            Value::Map(_) => write!(f, "{{...}}"),
            Value::Null => write!(f, "null"),
        }
    }
}

/// A nested key-value map used as evaluation context.
pub type KV = HashMap<String, Value>;

/// Evaluation context holding the key-value data.
pub struct Ctx {
    pub kv: KV,
}

/// Result of evaluating an expression.
pub struct EvalResult {
    pub value: Value,
    pub error: Option<EvalError>,
}

impl EvalResult {
    fn ok(&self) -> bool {
        self.error.is_none()
    }

    fn pass(&self) -> bool {
        self.ok() && !is_zero(&self.value)
    }

    fn fail(&self) -> bool {
        self.ok() && is_zero(&self.value)
    }

    fn with_value(value: Value) -> Self {
        EvalResult { value, error: None }
    }

    fn with_error(error: EvalError) -> Self {
        EvalResult {
            value: Value::Null,
            error: Some(error),
        }
    }
}

/// Check whether a value is "zero" (falsy).
pub fn is_zero(val: &Value) -> bool {
    match val {
        Value::Null => true,
        Value::Bool(b) => !b,
        Value::Int(n) => *n == 0,
        Value::Float(n) => *n == 0.0,
        Value::String(s) => s.is_empty(),
        Value::Bytes(b) | Value::HexString(b) => b.is_empty(),
        Value::Ip(_) => false,
        Value::IpCidr(_) => false,
        Value::Regex(_) => false,
        Value::Array(a) => a.is_empty(),
        Value::Map(m) => m.is_empty(),
    }
}

/// Look up a key in a KV map.
///
/// Supports dot-notation (e.g. "src.port") by first trying the full key,
/// then looking for the value at "src" and checking if it's a nested Map.
pub fn index_kv<'a>(kv: &'a KV, key: &str) -> Option<&'a Value> {
    // Direct lookup (handles both flat keys and pre-flattened dot-keys).
    if let Some(val) = kv.get(key) {
        return Some(val);
    }

    // Walk dot-separated path into nested Maps.
    if let Some(dot) = key.find('.') {
        let head = &key[..dot];
        let tail = &key[dot + 1..];
        if let Some(Value::Map(inner)) = kv.get(head) {
            return index_kv(inner, tail);
        }
    }

    None
}

/// Convert a parsed `LiteralValue` from the AST into a runtime `Value`.
fn literal_to_value(lit: &LiteralValue) -> Result<Value, EvalError> {
    match lit {
        LiteralValue::String(s) => Ok(Value::String(s.clone())),
        LiteralValue::Int(n) => Ok(Value::Int(*n)),
        LiteralValue::Float(n) => Ok(Value::Float(*n)),
        LiteralValue::Bool(b) => Ok(Value::Bool(*b)),
        LiteralValue::Ip(ip) => Ok(Value::Ip(*ip)),
        LiteralValue::IpCidr(s) => {
            let net: IpNet = s
                .parse()
                .map_err(|e| EvalError::InvalidOperation(format!("invalid CIDR {}: {}", s, e)))?;
            Ok(Value::IpCidr(net))
        }
        LiteralValue::HexString(bytes) => Ok(Value::HexString(bytes.clone())),
        LiteralValue::Regex(pattern) => {
            let re = Regex::new(pattern).map_err(|e| {
                EvalError::InvalidOperation(format!("invalid regex /{}/: {}", pattern, e))
            })?;
            Ok(Value::Regex(re))
        }
    }
}

impl Expr {
    /// Evaluate this expression against the given context.
    pub fn eval(&self, ctx: &Ctx) -> EvalResult {
        match self {
            Expr::And { left, right } => eval_and(left, right, ctx),
            Expr::Or { left, right } => eval_or(left, right, ctx),
            Expr::Not { expr } => eval_not(expr, ctx),
            Expr::Compare { left, op, right } => eval_compare(left, *op, right, ctx),
            Expr::Match { left, right } => eval_match(left, right, ctx),
            Expr::In { left, right } => eval_in(left, right, ctx),
            Expr::Field(name) => eval_field(name, ctx),
            Expr::Literal(lit) => match literal_to_value(lit) {
                Ok(val) => EvalResult::with_value(val),
                Err(e) => EvalResult::with_error(e),
            },
            Expr::Array(elems) => eval_array(elems, ctx),
            Expr::FunctionCall { name, .. } => EvalResult::with_error(
                EvalError::UnknownFunction(format!("{} (functions not yet implemented)", name)),
            ),
        }
    }
}

fn eval_and(left: &Expr, right: &Expr, ctx: &Ctx) -> EvalResult {
    let rleft = left.eval(ctx);
    if rleft.fail() {
        return rleft;
    }

    let rright = right.eval(ctx);
    if rright.fail() {
        return rright;
    }

    // If only one side has an error, return that side.
    if rleft.ok() && !rright.ok() {
        return rright;
    } else if !rleft.ok() && rright.ok() {
        return rleft;
    }

    let value = if rleft.ok() && rright.ok() {
        Value::Bool(rleft.pass() && rright.pass())
    } else {
        Value::Null
    };

    EvalResult {
        value,
        error: coalesce_errors(vec![rleft.error, rright.error]),
    }
}

fn eval_or(left: &Expr, right: &Expr, ctx: &Ctx) -> EvalResult {
    let rleft = left.eval(ctx);
    if rleft.pass() {
        return rleft;
    }

    let rright = right.eval(ctx);
    if rright.pass() {
        return rright;
    }

    if rleft.ok() && !rright.ok() {
        return rright;
    } else if !rleft.ok() && rright.ok() {
        return rleft;
    }

    let value = if rleft.ok() && rright.ok() {
        Value::Bool(rleft.pass() || rright.pass())
    } else {
        Value::Null
    };

    EvalResult {
        value,
        error: coalesce_errors(vec![rleft.error, rright.error]),
    }
}

fn eval_not(expr: &Expr, ctx: &Ctx) -> EvalResult {
    let r = expr.eval(ctx);
    if !r.ok() {
        return EvalResult::with_error(r.error.unwrap());
    }
    EvalResult::with_value(Value::Bool(is_zero(&r.value)))
}

fn eval_compare(left: &Expr, op: Operator, right: &Expr, ctx: &Ctx) -> EvalResult {
    let lv = left.eval(ctx);
    if !lv.ok() {
        return lv;
    }
    let rv = right.eval(ctx);
    if !rv.ok() {
        return rv;
    }
    let pass = compare::compare(&lv.value, op, &rv.value);
    EvalResult::with_value(Value::Bool(pass))
}

fn eval_match(left: &Expr, right: &Expr, ctx: &Ctx) -> EvalResult {
    let lv = left.eval(ctx);
    if !lv.ok() {
        return lv;
    }
    let rv = right.eval(ctx);
    if !rv.ok() {
        return rv;
    }
    let pass = compare::compare_match(&lv.value, &rv.value);
    EvalResult::with_value(Value::Bool(pass))
}

fn eval_in(left: &Expr, right: &Expr, ctx: &Ctx) -> EvalResult {
    let lv = left.eval(ctx);
    if !lv.ok() {
        return lv;
    }
    let rv = right.eval(ctx);
    if !rv.ok() {
        return rv;
    }

    let pass = match &rv.value {
        Value::Array(_) => {
            // `FIELD in ARR` == `ARR contains FIELD`
            compare::compare(&rv.value, Operator::Contains, &lv.value)
        }
        Value::IpCidr(net) => {
            // `IP in CIDR`
            match &lv.value {
                Value::Ip(ip) => net.contains(ip),
                _ => false,
            }
        }
        _ => false,
    };
    EvalResult::with_value(Value::Bool(pass))
}

fn eval_field(name: &str, ctx: &Ctx) -> EvalResult {
    match index_kv(&ctx.kv, name) {
        Some(val) => EvalResult::with_value(val.clone()),
        None => EvalResult::with_error(EvalError::MissingFields(
            [name.to_string()].into_iter().collect(),
        )),
    }
}

fn eval_array(elems: &[Expr], ctx: &Ctx) -> EvalResult {
    let mut vals = Vec::with_capacity(elems.len());
    for elem in elems {
        let r = elem.eval(ctx);
        if !r.ok() {
            return r;
        }
        vals.push(r.value);
    }
    EvalResult::with_value(Value::Array(vals))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_zero() {
        assert!(is_zero(&Value::Null));
        assert!(is_zero(&Value::Bool(false)));
        assert!(!is_zero(&Value::Bool(true)));
        assert!(is_zero(&Value::Int(0)));
        assert!(!is_zero(&Value::Int(1)));
        assert!(is_zero(&Value::Float(0.0)));
        assert!(!is_zero(&Value::Float(1.0)));
        assert!(is_zero(&Value::String("".into())));
        assert!(!is_zero(&Value::String("x".into())));
        assert!(is_zero(&Value::Array(vec![])));
        assert!(!is_zero(&Value::Array(vec![Value::Int(1)])));
    }

    #[test]
    fn test_index_kv_direct() {
        let mut kv = HashMap::new();
        kv.insert("port".into(), Value::Int(8080));
        assert!(matches!(index_kv(&kv, "port"), Some(Value::Int(8080))));
        assert!(index_kv(&kv, "missing").is_none());
    }

    #[test]
    fn test_eval_literal() {
        let ctx = Ctx { kv: HashMap::new() };
        let expr = Expr::Literal(LiteralValue::Int(42));
        let r = expr.eval(&ctx);
        assert!(r.ok());
        assert!(matches!(r.value, Value::Int(42)));
    }

    #[test]
    fn test_eval_field_lookup() {
        let mut kv = HashMap::new();
        kv.insert("port".into(), Value::Int(8080));
        let ctx = Ctx { kv };

        let expr = Expr::Field("port".into());
        let r = expr.eval(&ctx);
        assert!(r.ok());
        assert!(matches!(r.value, Value::Int(8080)));
    }

    #[test]
    fn test_eval_field_missing() {
        let ctx = Ctx { kv: HashMap::new() };
        let expr = Expr::Field("port".into());
        let r = expr.eval(&ctx);
        assert!(!r.ok());
        assert!(matches!(r.error, Some(EvalError::MissingFields(_))));
    }

    #[test]
    fn test_eval_compare_eq() {
        let mut kv = HashMap::new();
        kv.insert("port".into(), Value::Int(8080));
        let ctx = Ctx { kv };

        let expr = Expr::Compare {
            left: Box::new(Expr::Field("port".into())),
            op: Operator::Eq,
            right: Box::new(Expr::Literal(LiteralValue::Int(8080))),
        };
        let r = expr.eval(&ctx);
        assert!(r.pass());
    }

    #[test]
    fn test_eval_and_short_circuit() {
        let mut kv = HashMap::new();
        kv.insert("a".into(), Value::Bool(false));
        kv.insert("b".into(), Value::Bool(true));
        let ctx = Ctx { kv };

        let expr = Expr::And {
            left: Box::new(Expr::Field("a".into())),
            right: Box::new(Expr::Field("b".into())),
        };
        let r = expr.eval(&ctx);
        assert!(r.fail());
    }

    #[test]
    fn test_eval_or_short_circuit() {
        let mut kv = HashMap::new();
        kv.insert("a".into(), Value::Bool(true));
        let ctx = Ctx { kv };

        let expr = Expr::Or {
            left: Box::new(Expr::Field("a".into())),
            right: Box::new(Expr::Field("missing".into())),
        };
        let r = expr.eval(&ctx);
        assert!(r.pass());
    }

    #[test]
    fn test_eval_not() {
        let mut kv = HashMap::new();
        kv.insert("flag".into(), Value::Bool(false));
        let ctx = Ctx { kv };

        let expr = Expr::Not {
            expr: Box::new(Expr::Field("flag".into())),
        };
        let r = expr.eval(&ctx);
        assert!(r.pass());
    }

    #[test]
    fn test_eval_match() {
        let mut kv = HashMap::new();
        kv.insert("domain".into(), Value::String("example.com".into()));
        let ctx = Ctx { kv };

        let expr = Expr::Match {
            left: Box::new(Expr::Field("domain".into())),
            right: Box::new(Expr::Literal(LiteralValue::Regex(r"example\.com$".into()))),
        };
        let r = expr.eval(&ctx);
        assert!(r.pass());
    }

    #[test]
    fn test_eval_in_array() {
        let mut kv = HashMap::new();
        kv.insert("port".into(), Value::Int(443));
        let ctx = Ctx { kv };

        let expr = Expr::In {
            left: Box::new(Expr::Field("port".into())),
            right: Box::new(Expr::Array(vec![
                Expr::Literal(LiteralValue::Int(80)),
                Expr::Literal(LiteralValue::Int(443)),
                Expr::Literal(LiteralValue::Int(8080)),
            ])),
        };
        let r = expr.eval(&ctx);
        assert!(r.pass());
    }

    #[test]
    fn test_eval_in_cidr() {
        let mut kv = HashMap::new();
        kv.insert("ip".into(), Value::Ip("10.1.2.3".parse().unwrap()));
        let ctx = Ctx { kv };

        let expr = Expr::In {
            left: Box::new(Expr::Field("ip".into())),
            right: Box::new(Expr::Literal(LiteralValue::IpCidr("10.0.0.0/8".into()))),
        };
        let r = expr.eval(&ctx);
        assert!(r.pass());
    }
}
