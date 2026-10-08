//! Byte strings as rule values.
//!
//! A `Vec<u8>` field is a list of numbers unless it is marked
//! `#[rulekit(bytes)]` or it is one of the byte-string newtypes below.
//! [`bytes`] borrows a slice for `kv!`.

use std::borrow::Cow;

use crate::ast::Segment;
use crate::error::BoxError;
use crate::input_value::InputValue;
use crate::value::{Val, ValueRef};

/// A type that is a byte string, not a list of numbers.
pub trait ByteStr {
    /// The bytes, borrowed.
    fn as_byte_str(&self) -> &[u8];
}

impl ByteStr for [u8] {
    fn as_byte_str(&self) -> &[u8] {
        self
    }
}

impl ByteStr for Vec<u8> {
    fn as_byte_str(&self) -> &[u8] {
        self
    }
}

impl ByteStr for &[u8] {
    fn as_byte_str(&self) -> &[u8] {
        self
    }
}

impl<const N: usize> ByteStr for [u8; N] {
    fn as_byte_str(&self) -> &[u8] {
        self
    }
}

impl ByteStr for Box<[u8]> {
    fn as_byte_str(&self) -> &[u8] {
        self
    }
}

impl ByteStr for Cow<'_, [u8]> {
    fn as_byte_str(&self) -> &[u8] {
        self
    }
}

/// A borrowed byte string for [`kv!`](crate::kv) and other heterogeneous input.
///
/// `Vec<u8>` in `kv!` is a list of numbers. Wrap a slice with [`bytes`] when
/// the rule should see a byte string.
#[derive(Clone, Copy, Debug)]
pub struct Bytes<'a>(pub &'a [u8]);

/// Borrow `buf` as a byte-string input value.
pub fn bytes(buf: &[u8]) -> Bytes<'_> {
    Bytes(buf)
}

impl<C: ?Sized> InputValue<C> for Bytes<'_> {
    fn get<'a>(&'a self, _: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        if path.is_empty() {
            Ok(Some(Val::Ref(ValueRef::Bytes(self.0))))
        } else {
            Ok(None)
        }
    }
}

pub(crate) fn bytes_value<'a, T: ByteStr + ?Sized>(
    value: &'a T,
    path: &[Segment],
) -> Result<Option<Val<'a>>, BoxError> {
    if path.is_empty() {
        Ok(Some(Val::Ref(ValueRef::Bytes(value.as_byte_str()))))
    } else {
        Ok(None)
    }
}

#[cfg(feature = "bytes")]
impl ByteStr for bytes::Bytes {
    fn as_byte_str(&self) -> &[u8] {
        self
    }
}

#[cfg(feature = "bytes")]
impl ByteStr for bytes::BytesMut {
    fn as_byte_str(&self) -> &[u8] {
        self
    }
}

#[cfg(feature = "bytes")]
impl<C: ?Sized> InputValue<C> for bytes::Bytes {
    fn get<'a>(&'a self, _: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        bytes_value(self, path)
    }
}

#[cfg(feature = "bytes")]
impl<C: ?Sized> InputValue<C> for bytes::BytesMut {
    fn get<'a>(&'a self, _: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        bytes_value(self, path)
    }
}

#[cfg(feature = "serde_bytes")]
impl ByteStr for serde_bytes::ByteBuf {
    fn as_byte_str(&self) -> &[u8] {
        self
    }
}

#[cfg(feature = "serde_bytes")]
impl ByteStr for serde_bytes::Bytes {
    fn as_byte_str(&self) -> &[u8] {
        self
    }
}

#[cfg(feature = "serde_bytes")]
impl<const N: usize> ByteStr for serde_bytes::ByteArray<N> {
    fn as_byte_str(&self) -> &[u8] {
        self.as_ref()
    }
}

#[cfg(feature = "serde_bytes")]
impl<C: ?Sized> InputValue<C> for serde_bytes::ByteBuf {
    fn get<'a>(&'a self, _: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        bytes_value(self, path)
    }
}

#[cfg(feature = "serde_bytes")]
impl<C: ?Sized> InputValue<C> for serde_bytes::Bytes {
    fn get<'a>(&'a self, _: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        bytes_value(self, path)
    }
}

// `Bytes` is unsized, so the `&T` blanket (which requires `Sized`) does not
// cover a borrowed view. This is the type users store in a struct.
#[cfg(feature = "serde_bytes")]
impl<C: ?Sized> InputValue<C> for &serde_bytes::Bytes {
    fn get<'a>(&'a self, _: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        bytes_value(*self, path)
    }
}

#[cfg(feature = "serde_bytes")]
impl<C: ?Sized, const N: usize> InputValue<C> for serde_bytes::ByteArray<N> {
    fn get<'a>(&'a self, _: &'a C, path: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        bytes_value(self, path)
    }
}
