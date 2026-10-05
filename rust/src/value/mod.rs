//! Rule values.

mod ip;
mod mac;

pub use ip::{Cidr, Ip};
pub use mac::Mac;

/// An owned value: input data, a literal, or a function result.
#[derive(Clone, Debug)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Uint(u64),
    Float(f64),
    String(String),
    /// Byte strings, including hex literals.
    Bytes(Vec<u8>),
    Ip(Ip),
    Cidr(Cidr),
    Mac(Mac),
    Regex(Box<regex::Regex>),
    Array(Vec<Value>),
}

impl PartialEq for Value {
    /// Structural equality; regexes compare by pattern.
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Null, Value::Null) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Uint(a), Value::Uint(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Bytes(a), Value::Bytes(b)) => a == b,
            (Value::Ip(a), Value::Ip(b)) => a == b,
            (Value::Cidr(a), Value::Cidr(b)) => a == b,
            (Value::Mac(a), Value::Mac(b)) => a == b,
            (Value::Regex(a), Value::Regex(b)) => a.as_str() == b.as_str(),
            (Value::Array(a), Value::Array(b)) => a == b,
            _ => false,
        }
    }
}
