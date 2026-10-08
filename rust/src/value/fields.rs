//! Built-in fields of typed values (port of `valueField` in `fields.go`).

use super::query::query_field;
use super::{Val, ValueRef};

/// Resolve a field such as `url.host` or `ip.version`. `None` when the value
/// has no such field or the field is absent from the value.
pub(crate) fn value_field<'a>(value: ValueRef<'a>, key: &str) -> Option<Val<'a>> {
    let found = match value {
        ValueRef::Url(u) => match key {
            "scheme" => non_empty(u.scheme()),
            "host" => non_empty(u.host()),
            // Go strconv.Atoi: optional sign, decimal digits, int range.
            "port" => u.port().parse::<i64>().ok().map(ValueRef::Int),
            "path" => Some(ValueRef::Str(u.escaped_path())),
            "query" => Some(ValueRef::Query(u.raw_query())),
            "fragment" => non_empty(u.fragment()),
            "user" => u.username().and_then(non_empty),
            _ => None,
        },
        ValueRef::Query(raw) => return query_field(raw, key),
        ValueRef::Ip(ip) if key == "version" => Some(version(ip.is_v4())),
        ValueRef::Cidr(cidr) => match key {
            "network" => Some(ValueRef::Ip(cidr.network())),
            "prefix" => Some(ValueRef::Int(i64::from(cidr.prefix()))),
            "version" => Some(version(cidr.is_v4())),
            _ => None,
        },
        ValueRef::Mac(mac) if key == "oui" && mac.as_bytes().len() >= 3 => {
            Some(ValueRef::Mac(mac.oui()))
        }
        _ => None,
    };
    found.map(Val::Ref)
}

fn non_empty(s: &str) -> Option<ValueRef<'_>> {
    (!s.is_empty()).then_some(ValueRef::Str(s))
}

fn version(v4: bool) -> ValueRef<'static> {
    ValueRef::Str(if v4 { "v4" } else { "v6" })
}
