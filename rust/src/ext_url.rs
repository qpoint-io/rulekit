//! `url::Url` and `http::Uri` as input values.
//!
//! Field access borrows. `url::Url` comparison uses [`url::Url::as_str`]
//! (already normalized; the crate does not keep the original spelling).
//! `http::Uri` has no borrowed full text, so reading the URI itself (not a
//! field) allocates its `Display` form.

use crate::ast::Segment;
use crate::error::BoxError;
use crate::input_value::{InputValue, project};
use crate::value::{Val, Value, ValueRef};

fn non_empty(s: &str) -> Option<ValueRef<'_>> {
    (!s.is_empty()).then_some(ValueRef::Str(s))
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
        "scheme" => non_empty(url.scheme()),
        "host" => url.host_str().and_then(non_empty),
        // `url` omits default ports (443 on https). Digits that were written
        // and then dropped are not recoverable without the original string.
        "port" => url.port().map(|p| ValueRef::Int(i64::from(p))),
        "path" => Some(ValueRef::Str(url.path())),
        "query" => Some(ValueRef::Query(url.query().unwrap_or(""))),
        "fragment" => url.fragment().and_then(non_empty),
        "user" => non_empty(url.username()),
        _ => None,
    }?;
    project(Val::Ref(found), rest)
}

#[cfg(feature = "http")]
impl<C: ?Sized> InputValue<C> for http::Uri {
    fn get<'a>(&'a self, _: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        if path.is_empty() {
            // No borrowed full text. Measured in the report; field access does not take this path.
            return Ok(Some(Val::Owned(Value::String(self.to_string()))));
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
        "scheme" => uri.scheme().map(|s| s.as_str()).and_then(non_empty),
        "host" => uri.host().and_then(non_empty),
        "port" => uri.port_u16().map(|p| ValueRef::Int(i64::from(p))),
        "path" => Some(ValueRef::Str(uri.path())),
        "query" => Some(ValueRef::Query(uri.query().unwrap_or(""))),
        "user" => uri_user(uri),
        "fragment" => None,
        _ => None,
    }?;
    project(Val::Ref(found), rest)
}

#[cfg(feature = "http")]
fn uri_user(uri: &http::Uri) -> Option<ValueRef<'_>> {
    let auth = uri.authority()?.as_str();
    let (info, _) = auth.split_once('@')?;
    let user = info.split_once(':').map(|(u, _)| u).unwrap_or(info);
    non_empty(user)
}
