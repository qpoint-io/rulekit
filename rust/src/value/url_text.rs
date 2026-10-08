//! URL text form as borrowed pieces, so `http::Uri` can compare without
//! allocating a `String`. Scheme and host are lowercased, matching
//! [`Url::as_str`](super::Url::as_str).

/// Pieces of `scheme://userinfo@host:port` + path + `?query`.
///
/// Empty pieces are omitted, including the separators that introduce them.
#[derive(Clone, Copy, Debug)]
pub struct UrlText<'a> {
    /// Scheme, without `://`. Empty if the URI is relative.
    pub scheme: &'a str,
    /// User info before `@`, without the `@`. Empty if none.
    pub userinfo: &'a str,
    /// Host as it appears in the text, brackets included.
    pub host: &'a str,
    /// Port digits, without `:`. Empty if none.
    pub port: &'a str,
    /// Path as written.
    pub path: &'a str,
    /// Query without `?`. `None` if there is no `?`; `Some("")` if the query is empty.
    pub query: Option<&'a str>,
}

impl<'a> UrlText<'a> {
    /// Whether `other` equals this text form.
    pub(crate) fn eq_text(self, other: &str) -> bool {
        eq_bytes(UrlBytes::new(self), other.as_bytes())
    }

    /// Whether the text form contains `needle`.
    pub(crate) fn contains_text(self, needle: &str) -> bool {
        let needle = needle.as_bytes();
        if needle.is_empty() {
            return true;
        }
        let mut rest = UrlBytes::new(self);
        loop {
            if starts_with(rest.clone(), needle) {
                return true;
            }
            if rest.next().is_none() {
                return false;
            }
        }
    }

    /// The text form. Allocates; comparison does not use this.
    pub(crate) fn render(self) -> String {
        let mut out = String::new();
        for b in UrlBytes::new(self) {
            out.push(b as char);
        }
        out
    }
}

#[derive(Clone)]
struct UrlBytes<'a> {
    text: UrlText<'a>,
    stage: u8,
    index: usize,
}

impl<'a> UrlBytes<'a> {
    fn new(text: UrlText<'a>) -> Self {
        Self {
            text,
            stage: 0,
            index: 0,
        }
    }
}

impl Iterator for UrlBytes<'_> {
    type Item = u8;

    fn next(&mut self) -> Option<u8> {
        loop {
            // Stages 0-2 emit the piece then a following separator.
            // Stages 3 and 5 emit a leading separator then the piece.
            match self.stage {
                0 => {
                    if self.text.scheme.is_empty() {
                        self.advance();
                        continue;
                    }
                    if let Some(b) = take(self.text.scheme, true, b"://", &mut self.index) {
                        return Some(b);
                    }
                    self.advance();
                }
                1 => {
                    if self.text.userinfo.is_empty() {
                        self.advance();
                        continue;
                    }
                    if let Some(b) = take(self.text.userinfo, false, b"@", &mut self.index) {
                        return Some(b);
                    }
                    self.advance();
                }
                2 => {
                    if self.text.host.is_empty() {
                        self.advance();
                        continue;
                    }
                    if let Some(b) = take(self.text.host, true, b"", &mut self.index) {
                        return Some(b);
                    }
                    self.advance();
                }
                3 => {
                    if self.text.port.is_empty() {
                        self.advance();
                        continue;
                    }
                    if let Some(b) = take_prefixed(b":", self.text.port, &mut self.index) {
                        return Some(b);
                    }
                    self.advance();
                }
                4 => {
                    if let Some(b) = take(self.text.path, false, b"", &mut self.index) {
                        return Some(b);
                    }
                    self.advance();
                }
                5 => {
                    let query = self.text.query?;
                    if let Some(b) = take_prefixed(b"?", query, &mut self.index) {
                        return Some(b);
                    }
                    return None;
                }
                _ => return None,
            }
        }
    }
}

impl UrlBytes<'_> {
    fn advance(&mut self) {
        self.stage += 1;
        self.index = 0;
    }
}

/// Emit `src` (optionally lowercased) then `after`.
fn take(src: &str, lower: bool, after: &[u8], index: &mut usize) -> Option<u8> {
    if *index < src.len() {
        let b = src.as_bytes()[*index];
        *index += 1;
        return Some(if lower { b.to_ascii_lowercase() } else { b });
    }
    let i = *index - src.len();
    if i < after.len() {
        *index += 1;
        return Some(after[i]);
    }
    None
}

fn take_prefixed(prefix: &[u8], src: &str, index: &mut usize) -> Option<u8> {
    if *index < prefix.len() {
        let b = prefix[*index];
        *index += 1;
        return Some(b);
    }
    let i = *index - prefix.len();
    if i < src.len() {
        *index += 1;
        return Some(src.as_bytes()[i]);
    }
    None
}

fn eq_bytes(left: impl Iterator<Item = u8>, right: &[u8]) -> bool {
    let mut i = 0;
    for b in left {
        if right.get(i) != Some(&b) {
            return false;
        }
        i += 1;
    }
    i == right.len()
}

fn starts_with(mut bytes: impl Iterator<Item = u8>, prefix: &[u8]) -> bool {
    for &b in prefix {
        if bytes.next() != Some(b) {
            return false;
        }
    }
    true
}
