//! `url::Url` and `http::Uri` as input values.
//!
//! Field access borrows. A value compares as rulekit's own URL parsed from
//! `url.as_str()` or the URI's display form: those crates normalize first
//! (default ports, percent-encoding), so the original spelling is not recoverable.

use std::borrow::Cow;

use crate::ast::Segment;
use crate::error::BoxError;
use crate::input_value::{InputValue, project};
use crate::value::{UrlText, Val, Value, ValueRef};

fn non_empty(s: &str) -> Option<ValueRef<'_>> {
    (!s.is_empty()).then_some(ValueRef::Str(s))
}

/// Percent-decode like rulekit: keep the original when decoding changes
/// nothing or the result is not UTF-8. An owned string is only built when
/// the text actually changes, so plain fields stay allocation-free.
fn pct_decode(s: &str) -> Cow<'_, str> {
    if !s.as_bytes().contains(&b'%') {
        return Cow::Borrowed(s);
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && bytes[i + 1].is_ascii_hexdigit()
            && bytes[i + 2].is_ascii_hexdigit()
        {
            out.push(hex(bytes[i + 1]) << 4 | hex(bytes[i + 2]));
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    match String::from_utf8(out) {
        Ok(text) if text != s => Cow::Owned(text),
        _ => Cow::Borrowed(s),
    }
}

fn hex(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => 0,
    }
}

fn decoded_str(s: &str) -> Option<Val<'_>> {
    match pct_decode(s) {
        Cow::Borrowed(s) => non_empty(s).map(Val::Ref),
        Cow::Owned(s) if s.is_empty() => None,
        Cow::Owned(s) => Some(Val::Owned(Value::String(s))),
    }
}

/// rulekit strips IPv6 brackets from the host field.
fn bare_host(host: &str) -> &str {
    host.strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(host)
}

fn host_field(host: &str) -> Option<ValueRef<'_>> {
    non_empty(bare_host(host))
}

/// rulekit lowercases scheme and host. Borrow when already lowercase.
fn lower_str(s: &str) -> Val<'_> {
    if s.bytes().any(|b| b.is_ascii_uppercase()) {
        Val::Owned(Value::String(s.to_ascii_lowercase()))
    } else {
        Val::Ref(ValueRef::Str(s))
    }
}

fn lower_host(host: &str) -> Option<Val<'_>> {
    let host = bare_host(host);
    if host.is_empty() {
        return None;
    }
    Some(lower_str(host))
}

#[cfg(feature = "url")]
impl<C: ?Sized> InputValue<C> for url::Url {
    fn get<'a>(&'a self, _: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        Ok(url_at(self, path))
    }
}

#[cfg(feature = "url")]
fn url_at<'a>(url: &'a url::Url, path: &[Segment]) -> Option<Val<'a>> {
    let Some((head, rest)) = path.split_first() else {
        return Some(Val::Ref(ValueRef::Str(url.as_str())));
    };
    let Segment::Key { key, .. } = head else {
        return None;
    };
    let found = match key.as_str() {
        "scheme" => non_empty(url.scheme()).map(Val::Ref),
        "host" => url.host_str().and_then(host_field).map(Val::Ref),
        // Port digits as written in `as_str()`. The url crate drops default
        // ports from that serialization, which is what rulekit then parses.
        "port" => url.port().map(|p| Val::Ref(ValueRef::Int(i64::from(p)))),
        "path" => Some(Val::Ref(ValueRef::Str(url.path()))),
        "query" => Some(Val::Ref(ValueRef::Query(url.query().unwrap_or("")))),
        "fragment" => url.fragment().and_then(decoded_str),
        "user" => decoded_str(url.username()),
        _ => None,
    }?;
    project(found, rest)
}

#[cfg(feature = "http")]
impl<C: ?Sized> InputValue<C> for http::Uri {
    fn get<'a>(&'a self, _: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        if path.is_empty() {
            return Ok(Some(Val::Ref(ValueRef::UrlText(uri_text(self)))));
        }
        Ok(uri_at(self, path))
    }
}

#[cfg(feature = "http")]
fn uri_at<'a>(uri: &'a http::Uri, path: &[Segment]) -> Option<Val<'a>> {
    let (head, rest) = path.split_first()?;
    let Segment::Key { key, .. } = head else {
        return None;
    };
    let found = match key.as_str() {
        "scheme" => uri
            .scheme()
            .map(|s| s.as_str())
            .filter(|s| !s.is_empty())
            .map(lower_str),
        "host" => uri.host().and_then(lower_host),
        "port" => uri
            .port_u16()
            .map(|p| Val::Ref(ValueRef::Int(i64::from(p)))),
        "path" => Some(Val::Ref(ValueRef::Str(uri.path()))),
        "query" => Some(Val::Ref(ValueRef::Query(uri.query().unwrap_or("")))),
        "user" => uri_user(uri).and_then(decoded_str),
        "fragment" => None,
        _ => None,
    }?;
    project(found, rest)
}

#[cfg(feature = "http")]
fn uri_user(uri: &http::Uri) -> Option<&str> {
    let auth = uri.authority()?.as_str();
    let (info, _) = auth.split_once('@')?;
    let user = info.split_once(':').map(|(u, _)| u).unwrap_or(info);
    (!user.is_empty()).then_some(user)
}

#[cfg(feature = "http")]
fn uri_text(uri: &http::Uri) -> UrlText<'_> {
    let scheme = uri.scheme_str().unwrap_or("");
    let auth = uri.authority().map(|a| a.as_str()).unwrap_or("");
    let (userinfo, hostport) = match auth.split_once('@') {
        Some((user, host)) => (user, host),
        None => ("", auth),
    };
    let (host, port) = split_host_port(hostport);
    UrlText {
        scheme,
        userinfo,
        host,
        port,
        path: uri.path(),
        query: uri.query(),
    }
}

#[cfg(feature = "http")]
fn split_host_port(hostport: &str) -> (&str, &str) {
    if hostport.starts_with('[')
        && let Some(end) = hostport.find(']')
    {
        let host = &hostport[..=end];
        let rest = &hostport[end + 1..];
        let port = rest.strip_prefix(':').unwrap_or("");
        return (host, port);
    }
    match hostport.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => {
            (host, port)
        }
        _ => (hostport, ""),
    }
}
