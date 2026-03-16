use std::cmp::Ordering;

use regex::Regex;

use crate::ast::Operator;
use crate::eval::Value;

/// Compare two runtime values using the given operator.
pub fn compare(left: &Value, op: Operator, right: &Value) -> bool {
    // If right is an array, compare left against each element.
    if let Value::Array(arr) = right {
        if op == Operator::Contains {
            // `contains` does not support arrays on the right side.
            return false;
        }
        return compare_slice(arr, op, |el, op| compare(left, op, el));
    }

    match (left, right) {
        // --- string × ? ---
        (Value::String(l), Value::String(r)) => compare_strings(l, op, r),
        (Value::String(l), Value::Regex(r)) => compare_string_regex(l, op, r),
        (Value::String(l), Value::Ip(r)) => compare_strings(l, op, &r.to_string()),
        (Value::String(l), Value::IpCidr(r)) => compare_strings(l, op, &r.to_string()),
        (Value::String(l), Value::HexString(r)) => compare_bytes(l.as_bytes(), op, r),

        // --- number × number ---
        (Value::Int(_) | Value::Float(_), Value::Int(_) | Value::Float(_)) => {
            compare_numbers(left, op, right)
        }

        // --- bool × bool ---
        (Value::Bool(l), Value::Bool(r)) => match op {
            Operator::Eq => l == r,
            Operator::Ne => l != r,
            _ => false,
        },

        // --- IP × ? ---
        (Value::Ip(l), Value::Ip(r)) => match op {
            Operator::Eq => l == r,
            Operator::Ne => l != r,
            _ => false,
        },
        (Value::Ip(l), Value::IpCidr(r)) => match op {
            Operator::Eq | Operator::Contains => r.contains(l),
            Operator::Ne => !r.contains(l),
            _ => false,
        },
        (Value::IpCidr(l), Value::Ip(r)) => match op {
            Operator::Eq | Operator::Contains => l.contains(r),
            Operator::Ne => !l.contains(r),
            _ => false,
        },

        // --- bytes × bytes ---
        (Value::Bytes(l), Value::Bytes(r)) => compare_bytes(l, op, r),
        (Value::Bytes(l), Value::HexString(r)) => {
            compare_strings(&hex_lower(l), op, &hex_lower(r))
        }
        (Value::Bytes(l), Value::String(r)) => {
            compare_strings(&hex_lower(l), op, &r.to_lowercase())
        }

        // --- array × ? (left is array) ---
        (Value::Array(arr), _) => compare_slice(arr, op, |el, op| compare(el, op, right)),

        _ => false,
    }
}

/// Apply regex matching for the `matches` operator.
pub fn compare_match(left: &Value, right: &Value) -> bool {
    let re = match right {
        Value::Regex(r) => r,
        _ => return false,
    };

    match left {
        Value::String(s) => re.is_match(s),
        Value::Array(arr) => arr.iter().any(|el| compare_match(el, right)),
        _ => false,
    }
}

// --- helpers ---

fn compare_strings(left: &str, op: Operator, right: &str) -> bool {
    match op {
        Operator::Eq => left == right,
        Operator::Ne => left != right,
        Operator::Contains => left.contains(right),
        _ => false,
    }
}

fn compare_string_regex(left: &str, op: Operator, right: &Regex) -> bool {
    match op {
        Operator::Eq | Operator::Contains => right.is_match(left),
        Operator::Ne => !right.is_match(left),
        _ => false,
    }
}

fn compare_numbers(left: &Value, op: Operator, right: &Value) -> bool {
    let ord = cmp_numbers(left, right);
    match ord {
        Some(ord) => match op {
            Operator::Eq => ord == Ordering::Equal,
            Operator::Ne => ord != Ordering::Equal,
            Operator::Gt => ord == Ordering::Greater,
            Operator::Ge => ord != Ordering::Less,
            Operator::Lt => ord == Ordering::Less,
            Operator::Le => ord != Ordering::Greater,
            _ => false,
        },
        None => false,
    }
}

fn cmp_numbers(left: &Value, right: &Value) -> Option<Ordering> {
    match (left, right) {
        (Value::Int(l), Value::Int(r)) => Some(l.cmp(r)),
        (Value::Float(l), Value::Float(r)) => l.partial_cmp(r),
        (Value::Int(l), Value::Float(r)) => (*l as f64).partial_cmp(r),
        (Value::Float(l), Value::Int(r)) => l.partial_cmp(&(*r as f64)),
        _ => None,
    }
}

fn compare_bytes(left: &[u8], op: Operator, right: &[u8]) -> bool {
    match op {
        Operator::Eq => left == right,
        Operator::Ne => left != right,
        Operator::Contains => {
            // subsequence search
            left.windows(right.len()).any(|w| w == right)
        }
        _ => false,
    }
}

fn compare_slice(slice: &[Value], op: Operator, f: impl Fn(&Value, Operator) -> bool) -> bool {
    if op == Operator::Ne {
        // NE on a slice: none of the elements are equal
        return !compare_slice(slice, Operator::Eq, f);
    }

    let effective_op = if op == Operator::Contains {
        Operator::Eq
    } else {
        op
    };

    slice.iter().any(|el| f(el, effective_op))
}

fn hex_lower(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<Vec<_>>()
        .join(":")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_string_eq() {
        let l = Value::String("hello".into());
        let r = Value::String("hello".into());
        assert!(compare(&l, Operator::Eq, &r));
        assert!(!compare(&l, Operator::Ne, &r));
    }

    #[test]
    fn test_string_contains() {
        let l = Value::String("hello world".into());
        let r = Value::String("world".into());
        assert!(compare(&l, Operator::Contains, &r));
        assert!(!compare(&l, Operator::Contains, &Value::String("xyz".into())));
    }

    #[test]
    fn test_int_comparisons() {
        let l = Value::Int(10);
        let r = Value::Int(5);
        assert!(compare(&l, Operator::Gt, &r));
        assert!(compare(&l, Operator::Ge, &r));
        assert!(!compare(&l, Operator::Lt, &r));
        assert!(compare(&l, Operator::Ne, &r));
        assert!(!compare(&l, Operator::Eq, &r));
    }

    #[test]
    fn test_int_float_cross() {
        let l = Value::Int(10);
        let r = Value::Float(10.0);
        assert!(compare(&l, Operator::Eq, &r));
        assert!(compare(&l, Operator::Ge, &r));
        assert!(!compare(&l, Operator::Gt, &r));
    }

    #[test]
    fn test_bool_eq() {
        assert!(compare(&Value::Bool(true), Operator::Eq, &Value::Bool(true)));
        assert!(!compare(&Value::Bool(true), Operator::Eq, &Value::Bool(false)));
        assert!(compare(&Value::Bool(true), Operator::Ne, &Value::Bool(false)));
    }

    #[test]
    fn test_ip_eq() {
        let l = Value::Ip("192.168.1.1".parse().unwrap());
        let r = Value::Ip("192.168.1.1".parse().unwrap());
        assert!(compare(&l, Operator::Eq, &r));
        assert!(!compare(&l, Operator::Ne, &r));
    }

    #[test]
    fn test_ip_in_cidr() {
        let ip = Value::Ip("192.168.1.50".parse().unwrap());
        let cidr = Value::IpCidr("192.168.1.0/24".parse().unwrap());
        assert!(compare(&ip, Operator::Eq, &cidr));
        assert!(!compare(&ip, Operator::Ne, &cidr));

        let outside = Value::Ip("10.0.0.1".parse().unwrap());
        assert!(!compare(&outside, Operator::Eq, &cidr));
        assert!(compare(&outside, Operator::Ne, &cidr));
    }

    #[test]
    fn test_cidr_contains_ip() {
        let cidr = Value::IpCidr("10.0.0.0/8".parse().unwrap());
        let ip = Value::Ip("10.1.2.3".parse().unwrap());
        assert!(compare(&cidr, Operator::Contains, &ip));
    }

    #[test]
    fn test_string_regex() {
        let l = Value::String("example.com".into());
        let r = Value::Regex(Regex::new(r"example\.com$").unwrap());
        assert!(compare(&l, Operator::Eq, &r));
        assert!(!compare(&l, Operator::Ne, &r));

        let l2 = Value::String("other.org".into());
        assert!(!compare(&l2, Operator::Eq, &r));
        assert!(compare(&l2, Operator::Ne, &r));
    }

    #[test]
    fn test_array_contains() {
        let arr = Value::Array(vec![
            Value::String("a".into()),
            Value::String("b".into()),
            Value::String("c".into()),
        ]);
        let val = Value::String("b".into());
        assert!(compare(&arr, Operator::Contains, &val));
        assert!(!compare(&arr, Operator::Contains, &Value::String("d".into())));
    }

    #[test]
    fn test_value_in_array() {
        let val = Value::Int(2);
        let arr = Value::Array(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
        // "val in arr" means compare(val, op, arr) where arr is on the right
        // but the In node flips it: compare(arr, Contains, val)
        assert!(compare(&val, Operator::Eq, &arr));
    }

    #[test]
    fn test_compare_match() {
        let re = Value::Regex(Regex::new(r"\.com$").unwrap());
        assert!(compare_match(&Value::String("example.com".into()), &re));
        assert!(!compare_match(&Value::String("example.org".into()), &re));
    }

    #[test]
    fn test_bytes_eq() {
        let l = Value::Bytes(vec![0x01, 0x02]);
        let r = Value::Bytes(vec![0x01, 0x02]);
        assert!(compare(&l, Operator::Eq, &r));

        let r2 = Value::Bytes(vec![0x01, 0x03]);
        assert!(!compare(&l, Operator::Eq, &r2));
        assert!(compare(&l, Operator::Ne, &r2));
    }
}
