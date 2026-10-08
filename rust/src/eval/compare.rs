//! Comparisons (port of `compare*.go`).

use std::cmp::Ordering;

use crate::value::{ArrayRef, Cidr, Ip, Mac, UrlText, ValueRef};

/// Comparison operators (`in` is handled by the `In` node).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CmpOp {
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
    Contains,
}

impl CmpOp {
    /// Printed spelling (`==`, `contains`, ...).
    pub(crate) fn symbol(self) -> &'static str {
        match self {
            CmpOp::Eq => "==",
            CmpOp::Ne => "!=",
            CmpOp::Gt => ">",
            CmpOp::Ge => ">=",
            CmpOp::Lt => "<",
            CmpOp::Le => "<=",
            CmpOp::Contains => "contains",
        }
    }

    /// Machine name (`eq`, `contains`, ...).
    pub(crate) fn name(self) -> &'static str {
        match self {
            CmpOp::Eq => "eq",
            CmpOp::Ne => "ne",
            CmpOp::Gt => "gt",
            CmpOp::Ge => "ge",
            CmpOp::Lt => "lt",
            CmpOp::Le => "le",
            CmpOp::Contains => "contains",
        }
    }
}

/// Why a comparison could not be made (reported in traces).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Diagnostic {
    #[default]
    None,
    Incomparable,
    InvalidShape,
    UnsupportedOperator,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Outcome {
    pub pass: bool,
    pub diagnostic: Diagnostic,
}

fn pass(pass: bool) -> Outcome {
    Outcome {
        pass,
        diagnostic: Diagnostic::None,
    }
}

fn diagnostic(diagnostic: Diagnostic) -> Outcome {
    Outcome {
        pass: false,
        diagnostic,
    }
}

fn incomparable() -> Outcome {
    diagnostic(Diagnostic::Incomparable)
}

fn unsupported() -> Outcome {
    diagnostic(Diagnostic::UnsupportedOperator)
}

/// Go `compareDetailed`: the right side decides list handling first, then
/// the left value's type picks the comparison.
pub(crate) fn compare(left: ValueRef<'_>, op: CmpOp, right: ValueRef<'_>) -> Outcome {
    if let ValueRef::Array(items) = right {
        if op == CmpOp::Contains {
            // contains does not take a list on the right.
            return diagnostic(Diagnostic::InvalidShape);
        }
        return compare_slice(items, op, |rv, op| compare(left, op, rv));
    }
    match left {
        ValueRef::Str(s) | ValueRef::Query(s) => compare_string(s, op, right),
        ValueRef::Int(_) | ValueRef::Uint(_) | ValueRef::Float(_) => {
            compare_number(left, op, right)
        }
        ValueRef::Bool(l) => match right {
            ValueRef::Bool(r) => match op {
                CmpOp::Eq => pass(l == r),
                CmpOp::Ne => pass(l != r),
                _ => unsupported(),
            },
            _ => incomparable(),
        },
        ValueRef::Ip(ip) => compare_ip(ip, op, right),
        ValueRef::Cidr(cidr) => compare_cidr(cidr, op, right),
        ValueRef::Mac(mac) => compare_mac(mac, op, right),
        ValueRef::Url(url) => match right {
            ValueRef::Url(r) => compare_strings(url.as_str(), op, r.as_str()),
            ValueRef::UrlText(t) => compare_url_text(t, op, url.as_str()),
            ValueRef::Str(_) | ValueRef::Regex(_) | ValueRef::Query(_) => {
                compare_string(url.as_str(), op, right)
            }
            _ => incomparable(),
        },
        ValueRef::UrlText(text) => compare_url_text_value(text, op, right),
        ValueRef::Bytes(b) => compare_bytes(b, op, right),
        ValueRef::Array(items) => compare_slice(items, op, |lv, op| compare(lv, op, right)),
        ValueRef::Null | ValueRef::Regex(_) | ValueRef::Object(_) => incomparable(),
    }
}

/// Go `compareSliceDetailed`: `!=` passes when no element is equal;
/// `contains` on a list is element equality; otherwise any element passing
/// passes. The first diagnostic is kept when no element was comparable.
pub(crate) fn compare_slice(
    items: ArrayRef<'_>,
    op: CmpOp,
    f: impl Fn(ValueRef<'_>, CmpOp) -> Outcome,
) -> Outcome {
    if op == CmpOp::Ne {
        let mut eq = compare_slice(items, CmpOp::Eq, f);
        eq.pass = !eq.pass;
        return eq;
    }
    let op = if op == CmpOp::Contains { CmpOp::Eq } else { op };
    let mut first = Diagnostic::None;
    let mut comparable = false;
    for item in items.iter() {
        let outcome = f(item, op);
        if outcome.diagnostic == Diagnostic::None {
            comparable = true;
        } else if first == Diagnostic::None {
            first = outcome.diagnostic;
        }
        if outcome.pass {
            return pass(true);
        }
    }
    if comparable || items.is_empty() {
        return pass(false);
    }
    diagnostic(first)
}

fn compare_string(left: &str, op: CmpOp, right: ValueRef<'_>) -> Outcome {
    match right {
        ValueRef::Str(r) | ValueRef::Query(r) => compare_strings(left, op, r),
        ValueRef::UrlText(t) => compare_url_text(t, op, left),
        ValueRef::Regex(re) => match op {
            CmpOp::Eq | CmpOp::Contains => pass(re.is_match(left)),
            CmpOp::Ne => pass(!re.is_match(left)),
            _ => unsupported(),
        },
        ValueRef::Ip(_) | ValueRef::Cidr(_) | ValueRef::Url(_) | ValueRef::Mac(_) => {
            let text = right.text().expect("network values have a text form");
            compare_strings(left, op, &text)
        }
        ValueRef::Bytes(b) => compare_byte_slices(left.as_bytes(), op, b),
        _ => incomparable(),
    }
}

fn compare_url_text_value(text: UrlText<'_>, op: CmpOp, right: ValueRef<'_>) -> Outcome {
    match right {
        ValueRef::Str(s) | ValueRef::Query(s) => compare_url_text(text, op, s),
        ValueRef::Url(u) => compare_url_text(text, op, u.as_str()),
        ValueRef::UrlText(r) => {
            // Both sides are parts. Compare via the right-hand text only when
            // it is short enough to borrow... fall back to owned text forms.
            // Equality of two http URIs is rare; build both texts.
            let left = text.render();
            let right = r.render();
            compare_strings(&left, op, &right)
        }
        ValueRef::Regex(re) => {
            let owned = text.render();
            match op {
                CmpOp::Eq | CmpOp::Contains => pass(re.is_match(&owned)),
                CmpOp::Ne => pass(!re.is_match(&owned)),
                _ => unsupported(),
            }
        }
        _ => incomparable(),
    }
}

fn compare_url_text(text: UrlText<'_>, op: CmpOp, right: &str) -> Outcome {
    match op {
        CmpOp::Eq => pass(text.eq_text(right)),
        CmpOp::Ne => pass(!text.eq_text(right)),
        CmpOp::Contains => pass(text.contains_text(right)),
        _ => unsupported(),
    }
}

fn compare_strings(left: &str, op: CmpOp, right: &str) -> Outcome {
    match op {
        CmpOp::Eq => pass(left == right),
        CmpOp::Ne => pass(left != right),
        CmpOp::Contains => pass(left.contains(right)),
        _ => unsupported(),
    }
}

/// Go `compareIP`.
fn compare_ip(left: Ip, op: CmpOp, right: ValueRef<'_>) -> Outcome {
    match right {
        ValueRef::Ip(r) => match op {
            CmpOp::Eq => pass(left == r),
            CmpOp::Ne => pass(left != r),
            _ => unsupported(),
        },
        ValueRef::Cidr(net) => match op {
            CmpOp::Eq | CmpOp::Contains => pass(net.contains(left)),
            CmpOp::Ne => pass(!net.contains(left)),
            _ => unsupported(),
        },
        ValueRef::Str(_) | ValueRef::Regex(_) | ValueRef::Query(_) => {
            compare_string(&ValueRef::Ip(left).text().expect("text form"), op, right)
        }
        _ => incomparable(),
    }
}

/// Go `compareIPNet`.
fn compare_cidr(left: Cidr, op: CmpOp, right: ValueRef<'_>) -> Outcome {
    match right {
        ValueRef::Ip(ip) => match op {
            CmpOp::Eq | CmpOp::Contains => pass(left.contains(ip)),
            CmpOp::Ne => pass(!left.contains(ip)),
            _ => unsupported(),
        },
        ValueRef::Str(_) | ValueRef::Regex(_) | ValueRef::Query(_) => {
            compare_string(&ValueRef::Cidr(left).text().expect("text form"), op, right)
        }
        _ => incomparable(),
    }
}

/// Go `compareMac`.
fn compare_mac(left: Mac, op: CmpOp, right: ValueRef<'_>) -> Outcome {
    match right {
        ValueRef::Mac(r) => compare_byte_slices(left.as_bytes(), op, r.as_bytes()),
        ValueRef::Bytes(r) => compare_byte_slices(left.as_bytes(), op, r),
        ValueRef::Str(_) | ValueRef::Regex(_) | ValueRef::Query(_) => {
            compare_string(&ValueRef::Mac(left).text().expect("text form"), op, right)
        }
        _ => incomparable(),
    }
}

/// Go `compareBytes`: bytes and hex literals compare by value with bytes,
/// MACs, and strings.
fn compare_bytes(left: &[u8], op: CmpOp, right: ValueRef<'_>) -> Outcome {
    match right {
        ValueRef::Bytes(r) => compare_byte_slices(left, op, r),
        ValueRef::Mac(m) => compare_byte_slices(left, op, m.as_bytes()),
        ValueRef::Str(s) => match op {
            CmpOp::Eq => pass(left == s.as_bytes()),
            CmpOp::Ne => pass(left != s.as_bytes()),
            _ => compare_byte_slices(left, op, s.as_bytes()),
        },
        _ => incomparable(),
    }
}

fn compare_byte_slices(left: &[u8], op: CmpOp, right: &[u8]) -> Outcome {
    match op {
        CmpOp::Eq => pass(left == right),
        CmpOp::Ne => pass(left != right),
        CmpOp::Contains => pass(right.is_empty() || left.windows(right.len()).any(|w| w == right)),
        _ => unsupported(),
    }
}

/// Go `compareNumber` + `compareWithOp`.
fn compare_number(left: ValueRef<'_>, op: CmpOp, right: ValueRef<'_>) -> Outcome {
    let Some(ord) = cmp_number(left, right) else {
        return incomparable();
    };
    match op {
        CmpOp::Eq => pass(ord == Ordering::Equal),
        CmpOp::Ne => pass(ord != Ordering::Equal),
        CmpOp::Gt => pass(ord == Ordering::Greater),
        CmpOp::Ge => pass(ord != Ordering::Less),
        CmpOp::Lt => pass(ord == Ordering::Less),
        CmpOp::Le => pass(ord != Ordering::Greater),
        CmpOp::Contains => unsupported(),
    }
}

/// Go `cmpNumber`: exact across int64/uint64, float comparisons convert the
/// integer to float64.
pub fn cmp_number(left: ValueRef<'_>, right: ValueRef<'_>) -> Option<Ordering> {
    Some(match (left, right) {
        (ValueRef::Int(l), ValueRef::Int(r)) => l.cmp(&r),
        (ValueRef::Int(l), ValueRef::Uint(r)) => {
            if l < 0 {
                Ordering::Less
            } else {
                (l as u64).cmp(&r)
            }
        }
        (ValueRef::Uint(l), ValueRef::Int(r)) => {
            if r < 0 {
                Ordering::Greater
            } else {
                l.cmp(&(r as u64))
            }
        }
        (ValueRef::Uint(l), ValueRef::Uint(r)) => l.cmp(&r),
        (ValueRef::Int(l), ValueRef::Float(r)) => cmp_float(l as f64, r),
        (ValueRef::Uint(l), ValueRef::Float(r)) => cmp_float(l as f64, r),
        (ValueRef::Float(l), ValueRef::Float(r)) => cmp_float(l, r),
        (ValueRef::Float(l), ValueRef::Int(r)) => cmp_float(l, r as f64),
        (ValueRef::Float(l), ValueRef::Uint(r)) => cmp_float(l, r as f64),
        _ => return None,
    })
}

/// Go `cmp.Compare` for floats: NaN is less than everything else and equal
/// to NaN.
fn cmp_float(l: f64, r: f64) -> Ordering {
    match (l.is_nan(), r.is_nan()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        (false, false) => l.partial_cmp(&r).expect("not NaN"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_cross_width() {
        let ord = |l, r| cmp_number(l, r).unwrap();
        assert_eq!(
            ord(ValueRef::Int(-1), ValueRef::Uint(u64::MAX)),
            Ordering::Less
        );
        assert_eq!(
            ord(ValueRef::Uint(u64::MAX), ValueRef::Int(i64::MAX)),
            Ordering::Greater
        );
        assert_eq!(ord(ValueRef::Int(5), ValueRef::Float(5.0)), Ordering::Equal);
        assert_eq!(
            ord(ValueRef::Float(f64::NAN), ValueRef::Float(f64::NAN)),
            Ordering::Equal
        );
        assert_eq!(
            ord(ValueRef::Float(f64::NAN), ValueRef::Int(0)),
            Ordering::Less
        );
        assert!(cmp_number(ValueRef::Int(1), ValueRef::Str("1")).is_none());
    }
}
