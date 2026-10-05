//! Text forms without allocation.

use std::fmt;
use std::ops::Deref;

/// Longest inline text form: an IPv6 CIDR (`ffff:...:ffff/128`) is 43 bytes.
const INLINE: usize = 48;

/// A value's text form, from [`ValueRef::text`](super::ValueRef::text):
/// borrowed, or formatted into an inline buffer. Dereferences to `str`.
#[derive(Clone, Copy)]
pub struct TextForm<'a>(Repr<'a>);

#[derive(Clone, Copy)]
enum Repr<'a> {
    /// Text borrowed from the value.
    Borrowed(&'a str),
    /// UTF-8 text formatted inline (IPs, CIDRs, MACs) in the first `len` bytes.
    Inline { buf: [u8; INLINE], len: u8 },
}

impl<'a> TextForm<'a> {
    /// Text borrowed from a value.
    pub(crate) fn borrowed(text: &'a str) -> Self {
        TextForm(Repr::Borrowed(text))
    }

    /// Format a short value (IP, CIDR, MAC) inline.
    pub(crate) fn display(value: impl fmt::Display) -> Self {
        let mut w = InlineWriter {
            buf: [0; INLINE],
            len: 0,
        };
        fmt::write(&mut w, format_args!("{value}")).expect("text form fits the inline buffer");
        TextForm(Repr::Inline {
            buf: w.buf,
            len: w.len as u8,
        })
    }

    /// The text.
    pub fn as_str(&self) -> &str {
        match &self.0 {
            Repr::Borrowed(s) => s,
            Repr::Inline { buf, len } => {
                std::str::from_utf8(&buf[..usize::from(*len)]).expect("formatted text is UTF-8")
            }
        }
    }
}

impl Deref for TextForm<'_> {
    type Target = str;
    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Debug for TextForm<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

struct InlineWriter {
    buf: [u8; INLINE],
    len: usize,
}

impl fmt::Write for InlineWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let end = self.len + s.len();
        if end > INLINE {
            return Err(fmt::Error);
        }
        self.buf[self.len..end].copy_from_slice(s.as_bytes());
        self.len = end;
        Ok(())
    }
}
