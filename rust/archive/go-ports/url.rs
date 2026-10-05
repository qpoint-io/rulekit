//! URLs: a port of the parts of Go `net/url` that rulekit uses — `url.Parse`,
//! `URL.String`, `EscapedPath`, `Hostname` and `Port` — as of Go 1.27.1 with
//! its default GODEBUG settings (`urlstrictcolons=1`: an `http`/`https` host
//! may contain at most one colon outside brackets).
//!
//! Go keeps the decoded path, fragment and user name as byte strings, so the
//! parser works on bytes; only the accessors convert to `str`.

use std::borrow::Cow;
use std::fmt;

/// A parsed URL. Every accessor borrows from data computed once by
/// [`Url::parse`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Url {
    /// The text form, followed by the parts that are not substrings of it.
    buf: Box<str>,
    text_end: usize,
    scheme: Span,
    host: Span,
    port: Span,
    path: Span,
    query: Span,
    fragment: Span,
    user: Option<Span>,
}

/// A byte range of `Url::buf`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Span {
    start: usize,
    end: usize,
}

impl Span {
    const EMPTY: Span = Span { start: 0, end: 0 };
}

impl Url {
    /// Go `url.Parse(s)`: the same accept/reject decisions. The error text is
    /// descriptive but not Go's.
    pub fn parse(s: &str) -> Result<Url, String> {
        let (before, frag) = match s.find('#') {
            Some(i) => (&s[..i], &s[i + 1..]),
            None => (s, ""),
        };
        let mut u = parse_reference(before)?;
        if !frag.is_empty() {
            let fragment = unescape(frag.as_bytes(), Mode::Fragment)?;
            // RawFragment is a hint kept only when it differs from the
            // default encoding (setFragment).
            if escape(&fragment, Mode::Fragment) != frag {
                u.raw_fragment = frag;
            }
            u.fragment = fragment.into_owned();
        }
        Ok(u.build())
    }

    /// The text form: Go's `String()` of the URL with its host lowercased.
    pub fn as_str(&self) -> &str {
        &self.buf[..self.text_end]
    }

    /// The scheme, lowercase; empty for a relative URL.
    pub fn scheme(&self) -> &str {
        self.get(self.scheme)
    }

    /// Go `strings.ToLower(u.Hostname())`: the host without port, IPv6
    /// brackets stripped (a zone is kept, decoded: `fe80::1%en0`).
    pub fn host(&self) -> &str {
        self.get(self.host)
    }

    /// Go `u.Port()`: the decimal digits after the host's last colon, or
    /// empty.
    pub fn port(&self) -> &str {
        self.get(self.port)
    }

    /// Go `u.EscapedPath()`.
    pub fn escaped_path(&self) -> &str {
        self.get(self.path)
    }

    /// Go `u.RawQuery`: the query as written, without the `?`.
    pub fn raw_query(&self) -> &str {
        self.get(self.query)
    }

    /// Go `u.Fragment`: the decoded fragment. Bytes that are not UTF-8 after
    /// decoding (`#%ff`) read as U+FFFD, one per byte.
    pub fn fragment(&self) -> &str {
        self.get(self.fragment)
    }

    /// Go `u.User.Username()`, decoded, or `None` when the URL has no user
    /// info (`u.User == nil`). Bytes that are not UTF-8 after decoding read
    /// as U+FFFD, one per byte.
    pub fn username(&self) -> Option<&str> {
        self.user.map(|s| self.get(s))
    }

    fn get(&self, s: Span) -> &str {
        &self.buf[s.start..s.end]
    }
}

impl fmt::Display for Url {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Go's `url.URL` fields as `parse` leaves them; decoded parts are bytes.
struct Parsed<'a> {
    scheme: String,
    opaque: &'a str,
    user: User,
    host: Vec<u8>,
    path: Vec<u8>,
    raw_path: &'a str,
    raw_query: &'a str,
    force_query: bool,
    omit_host: bool,
    fragment: Vec<u8>,
    raw_fragment: &'a str,
}

/// Go `parse(rawURL, false)`: everything before the `#`.
fn parse_reference(raw: &str) -> Result<Parsed<'_>, String> {
    if raw.bytes().any(|b| b < b' ' || b == 0x7f) {
        return Err("invalid control character in URL".into());
    }
    let mut u = Parsed {
        scheme: String::new(),
        opaque: "",
        user: None,
        host: Vec::new(),
        path: Vec::new(),
        raw_path: "",
        raw_query: "",
        force_query: false,
        omit_host: false,
        fragment: Vec::new(),
        raw_fragment: "",
    };
    if raw == "*" {
        u.path = b"*".to_vec();
        return Ok(u);
    }

    let (scheme, rest) = get_scheme(raw)?;
    u.scheme = scheme.to_ascii_lowercase();

    let mut rest = if rest.ends_with('?') && rest.bytes().filter(|&b| b == b'?').count() == 1 {
        u.force_query = true;
        &rest[..rest.len() - 1]
    } else {
        match rest.split_once('?') {
            Some((r, q)) => {
                u.raw_query = q;
                r
            }
            None => rest,
        }
    };

    if !rest.starts_with('/') {
        if !u.scheme.is_empty() {
            // A rootless path after a scheme is opaque.
            u.opaque = rest;
            return Ok(u);
        }
        if first_segment(rest).contains(':') {
            return Err("first path segment in URL cannot contain colon".into());
        }
    }

    if (!u.scheme.is_empty() || !rest.starts_with("///")) && rest.starts_with("//") {
        let (authority, path) = match rest[2..].find('/') {
            Some(i) => rest[2..].split_at(i),
            None => (&rest[2..], ""),
        };
        rest = path;
        let (user, host) = parse_authority(&u.scheme, authority)?;
        u.user = user;
        u.host = host;
    } else if !u.scheme.is_empty() && rest.starts_with('/') {
        u.omit_host = true;
    }

    // setPath: RawPath is a hint kept only when it differs from the default
    // encoding of the decoded path.
    u.path = unescape(rest.as_bytes(), Mode::Path)?.into_owned();
    if escape(&u.path, Mode::Path) != rest {
        u.raw_path = rest;
    }
    Ok(u)
}

/// Go `getScheme`: a leading `[a-zA-Z][a-zA-Z0-9+.-]*:`, if any.
fn get_scheme(raw: &str) -> Result<(&str, &str), String> {
    for (i, c) in raw.bytes().enumerate() {
        match c {
            b'a'..=b'z' | b'A'..=b'Z' => {}
            b'0'..=b'9' | b'+' | b'-' | b'.' if i > 0 => {}
            b':' if i == 0 => return Err("missing protocol scheme".into()),
            b':' => return Ok((&raw[..i], &raw[i + 1..])),
            _ => return Ok(("", raw)),
        }
    }
    Ok(("", raw))
}

/// The text before the first `/`.
fn first_segment(path: &str) -> &str {
    path.split('/').next().unwrap_or("")
}

/// Decoded user info: the user name and, when a `:` was present, the
/// password (Go `*Userinfo`; `None` is a nil `User`).
type User = Option<(Vec<u8>, Option<Vec<u8>>)>;

/// Go `parseAuthority`: `[userinfo@]host[:port]`, split at the last `@`.
fn parse_authority(scheme: &str, authority: &str) -> Result<(User, Vec<u8>), String> {
    let at = authority.rfind('@');
    let host = parse_host(scheme, at.map_or(authority, |i| &authority[i + 1..]))?;
    let Some(at) = at else {
        return Ok((None, host));
    };
    let userinfo = &authority[..at];
    if !valid_userinfo(userinfo) {
        return Err("invalid userinfo".into());
    }
    let user = match userinfo.split_once(':') {
        None => (
            unescape(userinfo.as_bytes(), Mode::UserPassword)?.into_owned(),
            None,
        ),
        Some((name, password)) => (
            unescape(name.as_bytes(), Mode::UserPassword)?.into_owned(),
            Some(unescape(password.as_bytes(), Mode::UserPassword)?.into_owned()),
        ),
    };
    Ok((Some(user), host))
}

/// Go `validUserinfo`: RFC 3986 userinfo characters, plus `@`. Escapes are
/// checked later by `unescape`.
fn valid_userinfo(s: &str) -> bool {
    s.bytes().all(|c| {
        c.is_ascii_alphanumeric()
            || matches!(
                c,
                b'-' | b'.'
                    | b'_'
                    | b':'
                    | b'~'
                    | b'!'
                    | b'$'
                    | b'&'
                    | b'\''
                    | b'('
                    | b')'
                    | b'*'
                    | b'+'
                    | b','
                    | b';'
                    | b'='
                    | b'%'
                    | b'@'
            )
    })
}

/// Go `parseHost`: `host[:port]` or `[ipv6[%25zone]][:port]`, decoded.
fn parse_host(scheme: &str, host: &str) -> Result<Vec<u8>, String> {
    match host.rfind('[') {
        Some(0) => {
            let Some(close) = host.rfind(']') else {
                return Err("missing ']' in host".into());
            };
            let colon_port = &host[close + 1..];
            if !valid_optional_port(colon_port.as_bytes()) {
                return Err(format!("invalid port {colon_port:?} after host"));
            }
            // RFC 6874: `%25` starts the zone, which may escape more freely
            // than the address.
            let literal = &host.as_bytes()[1..close];
            let mut out = vec![b'['];
            match find(literal, b"%25") {
                Some(z) => {
                    out.extend_from_slice(&unescape(&literal[..z], Mode::Host)?);
                    out.extend_from_slice(&unescape(&literal[z..], Mode::Zone)?);
                }
                None => out.extend_from_slice(&unescape(literal, Mode::Host)?),
            }
            // Only an IPv6 address (IPv4-mapped included) may be bracketed.
            match parse_addr(&out[1..]) {
                Some(Family::V6) => {}
                Some(Family::V4) => return Err("invalid IP-literal".into()),
                None => return Err("invalid host: not an IP address".into()),
            }
            out.push(b']');
            // The port is digits only, so decoding leaves it unchanged.
            out.extend_from_slice(colon_port.as_bytes());
            Ok(out)
        }
        Some(_) => Err("invalid IP-literal".into()),
        None => {
            if let Some(first) = host.find(':') {
                let last = host.rfind(':').unwrap_or(first);
                // urlstrictcolons=1: http(s) hosts keep the first colon as
                // the port separator, so a second colon is an invalid port.
                let i = if last != first && scheme != "http" && scheme != "https" {
                    last
                } else {
                    first
                };
                let colon_port = &host[i..];
                if !valid_optional_port(colon_port.as_bytes()) {
                    return Err(format!("invalid port {colon_port:?} after host"));
                }
            }
            Ok(unescape(host.as_bytes(), Mode::Host)?.into_owned())
        }
    }
}

/// Go `validOptionalPort`: empty or `:` followed by decimal digits.
fn valid_optional_port(port: &[u8]) -> bool {
    match port.split_first() {
        None => true,
        Some((b':', digits)) => digits.iter().all(u8::is_ascii_digit),
        Some(_) => false,
    }
}

/// Go `splitHostPort` (used by `Hostname` and `Port`): splits off a valid
/// `:port` and strips IPv6 brackets.
fn split_host_port(host_port: &[u8]) -> (&[u8], &[u8]) {
    let (mut host, mut port) = (host_port, &host_port[..0]);
    if let Some(colon) = host.iter().rposition(|&c| c == b':')
        && valid_optional_port(&host[colon..])
    {
        (host, port) = (&host_port[..colon], &host_port[colon + 1..]);
    }
    if host.len() >= 2 && host[0] == b'[' && host[host.len() - 1] == b']' {
        host = &host[1..host.len() - 1];
    }
    (host, port)
}

impl Parsed<'_> {
    fn build(self) -> Url {
        // Go `lower := *u; lower.Host = strings.ToLower(u.Host); lower.String()`.
        let lower_host = go_to_lower(&self.host);
        let mut buf = String::with_capacity(
            self.scheme.len()
                + self.opaque.len()
                + 3 * (lower_host.len() + self.path.len() + self.fragment.len())
                + self.raw_query.len()
                + 16,
        );
        let mut text_path = None;
        let mut query = Span::EMPTY;

        if !self.scheme.is_empty() {
            buf.push_str(&self.scheme);
            buf.push(':');
        }
        let escaped_path = self.escaped_path();
        if !self.opaque.is_empty() {
            buf.push_str(self.opaque);
        } else {
            let no_authority = self.omit_host && self.host.is_empty() && self.user.is_none();
            if (!self.scheme.is_empty() || !self.host.is_empty() || self.user.is_some())
                && !no_authority
            {
                if !self.host.is_empty() || !self.path.is_empty() || self.user.is_some() {
                    buf.push_str("//");
                }
                if let Some((name, password)) = &self.user {
                    escape_into(&mut buf, name, Mode::UserPassword);
                    if let Some(password) = password {
                        buf.push(':');
                        escape_into(&mut buf, password, Mode::UserPassword);
                    }
                    buf.push('@');
                }
                escape_into(&mut buf, lower_host.as_bytes(), Mode::Host);
            }
            let mut path = escaped_path.as_ref();
            if no_authority && path.starts_with("//") {
                // Keep a re-parse from reading the path as an authority.
                buf.push_str("%2F");
                path = &path[1..];
            }
            if !path.is_empty() && !path.starts_with('/') && !self.host.is_empty() {
                buf.push('/');
            }
            if buf.is_empty() && first_segment(path).contains(':') {
                // RFC 3986 §4.2: keep the first segment from reading as a
                // scheme.
                buf.push_str("./");
            }
            if path.len() == escaped_path.len() {
                text_path = Some(Span {
                    start: buf.len(),
                    end: buf.len() + path.len(),
                });
            }
            buf.push_str(path);
        }
        if self.force_query || !self.raw_query.is_empty() {
            buf.push('?');
            query = Span {
                start: buf.len(),
                end: buf.len() + self.raw_query.len(),
            };
            buf.push_str(self.raw_query);
        }
        if !self.fragment.is_empty() {
            buf.push('#');
            buf.push_str(&self.escaped_fragment());
        }
        let text_end = buf.len();

        let scheme = Span {
            start: 0,
            end: self.scheme.len(),
        };
        let path = match text_path {
            Some(span) => span,
            None => push(&mut buf, &escaped_path),
        };
        let (hostname, port) = split_host_port(&self.host);
        let host = push(&mut buf, &go_to_lower(hostname));
        let port = push(&mut buf, &go_lossy(port));
        let fragment = push(&mut buf, &go_lossy(&self.fragment));
        let user = self
            .user
            .as_ref()
            .map(|(name, _)| push(&mut buf, &go_lossy(name)));
        Url {
            buf: buf.into_boxed_str(),
            text_end,
            scheme,
            host,
            port,
            path,
            query,
            fragment,
            user,
        }
    }

    /// Go `EscapedPath`: RawPath when it is a valid encoding of Path, else
    /// Path encoded.
    fn escaped_path(&self) -> Cow<'_, str> {
        if !self.raw_path.is_empty()
            && valid_encoded(self.raw_path, Mode::Path)
            && unescape(self.raw_path.as_bytes(), Mode::Path).is_ok_and(|p| *p == *self.path)
        {
            return Cow::Borrowed(self.raw_path);
        }
        if self.path == b"*" {
            return Cow::Borrowed("*");
        }
        Cow::Owned(escape(&self.path, Mode::Path))
    }

    /// Go `EscapedFragment`, as `escaped_path`.
    fn escaped_fragment(&self) -> Cow<'_, str> {
        if !self.raw_fragment.is_empty()
            && valid_encoded(self.raw_fragment, Mode::Fragment)
            && unescape(self.raw_fragment.as_bytes(), Mode::Fragment)
                .is_ok_and(|f| *f == *self.fragment)
        {
            return Cow::Borrowed(self.raw_fragment);
        }
        Cow::Owned(escape(&self.fragment, Mode::Fragment))
    }
}

/// Appends `s` to `buf` and returns its span.
fn push(buf: &mut String, s: &str) -> Span {
    let start = buf.len();
    buf.push_str(s);
    Span {
        start,
        end: buf.len(),
    }
}

/// The URL component an escape or unescape applies to (Go's `encoding`
/// modes that `Parse` and `String` use).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Path,
    Host,
    Zone,
    UserPassword,
    Fragment,
}

/// Go `shouldEscape`.
fn should_escape(c: u8, mode: Mode) -> bool {
    if c.is_ascii_alphanumeric() {
        return false;
    }
    if matches!(mode, Mode::Host | Mode::Zone)
        && matches!(
            c,
            b'!' | b'$'
                | b'&'
                | b'\''
                | b'('
                | b')'
                | b'*'
                | b'+'
                | b','
                | b';'
                | b'='
                | b':'
                | b'['
                | b']'
                | b'<'
                | b'>'
                | b'"'
        )
    {
        return false;
    }
    match c {
        b'-' | b'_' | b'.' | b'~' => return false,
        b'$' | b'&' | b'+' | b',' | b'/' | b':' | b';' | b'=' | b'?' | b'@' => match mode {
            Mode::Path => return c == b'?',
            Mode::UserPassword => return matches!(c, b'@' | b'/' | b'?' | b':'),
            Mode::Fragment => return false,
            Mode::Host | Mode::Zone => {}
        },
        _ => {}
    }
    !(mode == Mode::Fragment && matches!(c, b'!' | b'(' | b')' | b'*'))
}

/// Go `validEncoded`: `s` has no byte that encoding would escape (sub-delims,
/// `:`, `@`, brackets and `%` are allowed as written).
fn valid_encoded(s: &str, mode: Mode) -> bool {
    s.bytes().all(|c| {
        matches!(
            c,
            b'!' | b'$'
                | b'&'
                | b'\''
                | b'('
                | b')'
                | b'*'
                | b'+'
                | b','
                | b';'
                | b'='
                | b':'
                | b'@'
                | b'['
                | b']'
                | b'%'
        ) || !should_escape(c, mode)
    })
}

/// Go `escape`. Every byte left as written is ASCII.
fn escape(s: &[u8], mode: Mode) -> String {
    let mut out = String::with_capacity(s.len());
    escape_into(&mut out, s, mode);
    out
}

fn escape_into(out: &mut String, s: &[u8], mode: Mode) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for &c in s {
        if should_escape(c, mode) {
            out.push('%');
            out.push(char::from(HEX[usize::from(c >> 4)]));
            out.push(char::from(HEX[usize::from(c & 15)]));
        } else {
            out.push(char::from(c));
        }
    }
}

fn unhex(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Go `unescape`: decodes `%XX`. A host may escape only non-ASCII bytes (and
/// `%25`) and must otherwise use host characters; a zone may escape only
/// bytes it could write directly, a space, or `%25`.
fn unescape(s: &[u8], mode: Mode) -> Result<Cow<'_, [u8]>, String> {
    let mut escapes = 0;
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if c == b'%' {
            let (Some(hi), Some(lo)) = (
                s.get(i + 1).copied().and_then(unhex),
                s.get(i + 2).copied().and_then(unhex),
            ) else {
                let bad = &s[i..s.len().min(i + 3)];
                return Err(format!(
                    "invalid URL escape {:?}",
                    String::from_utf8_lossy(bad)
                ));
            };
            let pct25 = hi == 2 && lo == 5;
            let v = (hi << 4) | lo;
            if (mode == Mode::Host && hi < 8 && !pct25)
                || (mode == Mode::Zone && !pct25 && v != b' ' && should_escape(v, Mode::Host))
            {
                return Err(format!(
                    "invalid URL escape {:?}",
                    String::from_utf8_lossy(&s[i..i + 3])
                ));
            }
            escapes += 1;
            i += 3;
        } else {
            if matches!(mode, Mode::Host | Mode::Zone)
                && c < 0x80
                && c != b'+'
                && should_escape(c, mode)
            {
                return Err(format!(
                    "invalid character {:?} in host name",
                    char::from(c)
                ));
            }
            i += 1;
        }
    }
    if escapes == 0 {
        return Ok(Cow::Borrowed(s));
    }
    let mut out = Vec::with_capacity(s.len() - 2 * escapes);
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'%' {
            // Validated above.
            let hi = unhex(s[i + 1]).unwrap_or(0);
            let lo = unhex(s[i + 2]).unwrap_or(0);
            out.push((hi << 4) | lo);
            i += 3;
        } else {
            out.push(s[i]);
            i += 1;
        }
    }
    Ok(Cow::Owned(out))
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Bytes as a Go program ranging over them sees them: UTF-8 kept, each byte
/// of an invalid sequence read as U+FFFD.
fn go_lossy(s: &[u8]) -> Cow<'_, str> {
    if let Ok(s) = std::str::from_utf8(s) {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len() + 8);
    for chunk in s.utf8_chunks() {
        out.push_str(chunk.valid());
        out.extend(chunk.invalid().iter().map(|_| char::REPLACEMENT_CHARACTER));
    }
    Cow::Owned(out)
}

/// Go `strings.ToLower`: ASCII-only input lowercases ASCII letters; anything
/// else maps every rune through `unicode.ToLower` (simple case mapping), each
/// byte of an invalid UTF-8 sequence becoming U+FFFD.
fn go_to_lower(s: &[u8]) -> String {
    if s.is_ascii() {
        let mut out = String::with_capacity(s.len());
        out.extend(s.iter().map(|&c| char::from(c.to_ascii_lowercase())));
        return out;
    }
    let s = go_lossy(s);
    let mut out = String::with_capacity(s.len());
    // `char::to_lowercase` is the full mapping; its first char is the simple
    // mapping (only U+0130 has a multi-char lowercase: `i` + U+0307).
    out.extend(s.chars().map(|c| c.to_lowercase().next().unwrap_or(c)));
    out
}

#[derive(Debug, PartialEq, Eq)]
enum Family {
    V4,
    V6,
}

/// Go `netip.ParseAddr` acceptance (zones allowed on IPv6): `None` when Go
/// fails, else whether the result is IPv4 (`Addr.Is4`).
fn parse_addr(s: &[u8]) -> Option<Family> {
    for &c in s {
        match c {
            b'.' => return parse_ipv4(s).map(|_| Family::V4),
            b':' => return parse_ipv6(s).then_some(Family::V6),
            b'%' => return None,
            _ => {}
        }
    }
    None
}

/// Go `parseIPv4Fields`: four decimal octets, no leading zeros.
fn parse_ipv4(s: &[u8]) -> Option<()> {
    let (mut val, mut digits, mut dots) = (0u32, 0, 0);
    for (i, &c) in s.iter().enumerate() {
        match c {
            b'0'..=b'9' => {
                if digits == 1 && val == 0 {
                    return None;
                }
                val = val * 10 + u32::from(c - b'0');
                digits += 1;
                if val > 255 {
                    return None;
                }
            }
            b'.' => {
                if i == 0 || i == s.len() - 1 || s[i - 1] == b'.' || dots == 3 {
                    return None;
                }
                dots += 1;
                val = 0;
                digits = 0;
            }
            _ => return None,
        }
    }
    (dots == 3).then_some(())
}

/// Go `parseIPv6` acceptance: hex groups of at most four digits, at most one
/// `::` standing for at least one zero group, an optional embedded IPv4 tail
/// in the last 32 bits, and an optional non-empty `%zone`.
fn parse_ipv6(input: &[u8]) -> bool {
    let mut s = input;
    if let Some(z) = s.iter().position(|&c| c == b'%') {
        if z + 1 == s.len() {
            return false;
        }
        s = &s[..z];
    }
    let mut ellipsis = false;
    if s.starts_with(b"::") {
        ellipsis = true;
        s = &s[2..];
        if s.is_empty() {
            return true;
        }
    }
    let mut i = 0;
    while i < 16 {
        let mut off = 0;
        let mut acc = 0u32;
        while let Some(d) = s.get(off).copied().and_then(unhex) {
            acc = (acc << 4) + u32::from(d);
            if off > 3 || acc > 0xffff {
                return false;
            }
            off += 1;
        }
        if off == 0 {
            return false;
        }
        if s.get(off) == Some(&b'.') {
            // Embedded IPv4: only in the last 32 bits.
            if (!ellipsis && i != 12) || i + 4 > 16 || parse_ipv4(s).is_none() {
                return false;
            }
            s = &[];
            i += 4;
            break;
        }
        i += 2;
        s = &s[off..];
        if s.is_empty() {
            break;
        }
        if s[0] != b':' || s.len() == 1 {
            return false;
        }
        s = &s[1..];
        if s[0] == b':' {
            if ellipsis {
                return false;
            }
            ellipsis = true;
            s = &s[1..];
            if s.is_empty() {
                break;
            }
        }
    }
    if !s.is_empty() {
        return false;
    }
    // Short addresses need a `::`; full ones must not have one.
    (i < 16) == ellipsis
}

#[cfg(test)]
mod tests {
    use super::Url;

    /// (input, text form, [scheme, host, port, escaped path, raw query,
    /// fragment], username), produced by Go 1.27.1 `net/url`.
    const ACCEPT: &[(&str, &str, [&str; 6], Option<&str>)] = &[
        ("", "", ["", "", "", "", "", ""], None),
        ("*", "*", ["", "", "", "*", "", ""], None),
        ("*#f", "*#f", ["", "", "", "*", "", "f"], None),
        ("*?", "*?", ["", "", "", "*", "", ""], None),
        ("http://h", "http://h", ["http", "h", "", "", "", ""], None),
        (
            "http://h?",
            "http://h?",
            ["http", "h", "", "", "", ""],
            None,
        ),
        (
            "http://h??",
            "http://h??",
            ["http", "h", "", "", "?", ""],
            None,
        ),
        (
            "http://h?a?",
            "http://h?a?",
            ["http", "h", "", "", "a?", ""],
            None,
        ),
        ("http://h#", "http://h", ["http", "h", "", "", "", ""], None),
        (
            "http://h?#",
            "http://h?",
            ["http", "h", "", "", "", ""],
            None,
        ),
        (
            "HTTPS://Example.COM:443/a%20b/../c?q=A+b&x=%41#Frag%20x",
            "https://example.com:443/a%20b/../c?q=A+b&x=%41#Frag%20x",
            [
                "https",
                "example.com",
                "443",
                "/a%20b/../c",
                "q=A+b&x=%41",
                "Frag x",
            ],
            None,
        ),
        (
            "http://[::1]:80/p",
            "http://[::1]:80/p",
            ["http", "::1", "80", "/p", "", ""],
            None,
        ),
        (
            "http://user:pw@h",
            "http://user:pw@h",
            ["http", "h", "", "", "", ""],
            Some("user"),
        ),
        (
            "mailto:a@b.com",
            "mailto:a@b.com",
            ["mailto", "", "", "", "", ""],
            None,
        ),
        (
            "http://h/a b",
            "http://h/a%20b",
            ["http", "h", "", "/a%20b", "", ""],
            None,
        ),
        (
            "mailto:a b%zz",
            "mailto:a b%zz",
            ["mailto", "", "", "", "", ""],
            None,
        ),
        ("http:", "http:", ["http", "", "", "", "", ""], None),
        ("http:?", "http:?", ["http", "", "", "", "", ""], None),
        ("http:/", "http:/", ["http", "", "", "/", "", ""], None),
        ("http:/a", "http:/a", ["http", "", "", "/a", "", ""], None),
        ("http://", "http:", ["http", "", "", "", "", ""], None),
        ("http:///", "http:///", ["http", "", "", "/", "", ""], None),
        (
            "http:////x",
            "http:////x",
            ["http", "", "", "//x", "", ""],
            None,
        ),
        ("//", "", ["", "", "", "", "", ""], None),
        ("///", "///", ["", "", "", "///", "", ""], None),
        ("////x", "////x", ["", "", "", "////x", "", ""], None),
        ("//h/p", "//h/p", ["", "h", "", "/p", "", ""], None),
        ("/a:b", "/a:b", ["", "", "", "/a:b", "", ""], None),
        ("a:b", "a:b", ["a", "", "", "", "", ""], None),
        ("a%3Ab", "a%3Ab", ["", "", "", "a%3Ab", "", ""], None),
        (
            "a%3Ab c",
            "./a:b%20c",
            ["", "", "", "a:b%20c", "", ""],
            None,
        ),
        ("./a:b", "./a:b", ["", "", "", "./a:b", "", ""], None),
        (
            "http:%2F/a",
            "http:%2F/a",
            ["http", "", "", "", "", ""],
            None,
        ),
        (
            "http:/%2Fa b",
            "http:%2F/a%20b",
            ["http", "", "", "//a%20b", "", ""],
            None,
        ),
        (
            "http:/%2F",
            "http:/%2F",
            ["http", "", "", "/%2F", "", ""],
            None,
        ),
        (
            "http://%ff/",
            "http://%EF%BF%BD/",
            ["http", "�", "", "/", "", ""],
            None,
        ),
        (
            "http://%c3%89/",
            "http://%C3%A9/",
            ["http", "é", "", "/", "", ""],
            None,
        ),
        (
            "http://ÉXAMPLE.com/",
            "http://%C3%A9xample.com/",
            ["http", "éxample.com", "", "/", "", ""],
            None,
        ),
        (
            "postgres://a:1,b:2/db",
            "postgres://a:1,b:2/db",
            ["postgres", "a:1,b", "2", "/db", "", ""],
            None,
        ),
        (
            "mongodb://a:1:2",
            "mongodb://a:1:2",
            ["mongodb", "a:1", "2", "", "", ""],
            None,
        ),
        (
            "http://[::1]",
            "http://[::1]",
            ["http", "::1", "", "", "", ""],
            None,
        ),
        (
            "http://[::1]:",
            "http://[::1]:",
            ["http", "::1", "", "", "", ""],
            None,
        ),
        (
            "http://[::ffff:1.2.3.4]/",
            "http://[::ffff:1.2.3.4]/",
            ["http", "::ffff:1.2.3.4", "", "/", "", ""],
            None,
        ),
        (
            "http://[fe80::1%25en0]:8080/",
            "http://[fe80::1%25en0]:8080/",
            ["http", "fe80::1%en0", "8080", "/", "", ""],
            None,
        ),
        (
            "http://[fe80::1%25EN0]/",
            "http://[fe80::1%25en0]/",
            ["http", "fe80::1%en0", "", "/", "", ""],
            None,
        ),
        (
            "http://[fe80::1%25%20x]/",
            "http://[fe80::1%25%20x]/",
            ["http", "fe80::1% x", "", "/", "", ""],
            None,
        ),
        (
            "http://[fe80::1%25%41]/",
            "http://[fe80::1%25a]/",
            ["http", "fe80::1%a", "", "/", "", ""],
            None,
        ),
        (
            "http://[fe80::1%25a<b]/",
            "http://[fe80::1%25a<b]/",
            ["http", "fe80::1%a<b", "", "/", "", ""],
            None,
        ),
        (
            "http://a%25b/",
            "http://a%25b/",
            ["http", "a%b", "", "/", "", ""],
            None,
        ),
        (
            "http://h<>/",
            "http://h<>/",
            ["http", "h<>", "", "/", "", ""],
            None,
        ),
        (
            "http://h\"/",
            "http://h\"/",
            ["http", "h\"", "", "/", "", ""],
            None,
        ),
        (
            "http://u@@h",
            "http://u%40@h",
            ["http", "h", "", "", "", ""],
            Some("u@"),
        ),
        (
            "http://a@b@h",
            "http://a%40b@h",
            ["http", "h", "", "", "", ""],
            Some("a@b"),
        ),
        (
            "http://u:p@ss@h",
            "http://u:p%40ss@h",
            ["http", "h", "", "", "", ""],
            Some("u"),
        ),
        (
            "http://%40@h",
            "http://%40@h",
            ["http", "h", "", "", "", ""],
            Some("@"),
        ),
        (
            "http://:@h",
            "http://:@h",
            ["http", "h", "", "", "", ""],
            Some(""),
        ),
        (
            "http://@h",
            "http://@h",
            ["http", "h", "", "", "", ""],
            Some(""),
        ),
        (
            "http://u:@h",
            "http://u:@h",
            ["http", "h", "", "", "", ""],
            Some("u"),
        ),
        (
            "http://u;x=1@h",
            "http://u;x=1@h",
            ["http", "h", "", "", "", ""],
            Some("u;x=1"),
        ),
        (
            "http://%ff@h",
            "http://%FF@h",
            ["http", "h", "", "", "", ""],
            Some("�"),
        ),
        (
            "http://h#%ff",
            "http://h#%ff",
            ["http", "h", "", "", "", "�"],
            None,
        ),
        (
            "http://h#a#b",
            "http://h#a%23b",
            ["http", "h", "", "", "", "a#b"],
            None,
        ),
        (
            "http://h#a b",
            "http://h#a%20b",
            ["http", "h", "", "", "", "a b"],
            None,
        ),
        (
            "http://h#\u{9}",
            "http://h#%09",
            ["http", "h", "", "", "", "\u{9}"],
            None,
        ),
        (
            "http://h#!*()'",
            "http://h#!*()'",
            ["http", "h", "", "", "", "!*()'"],
            None,
        ),
        (
            "http://h/!*()'",
            "http://h/!*()'",
            ["http", "h", "", "/!*()'", "", ""],
            None,
        ),
        (
            "http://h/[x]",
            "http://h/[x]",
            ["http", "h", "", "/[x]", "", ""],
            None,
        ),
        (
            "http://h/%2fa",
            "http://h/%2fa",
            ["http", "h", "", "/%2fa", "", ""],
            None,
        ),
        (
            "http://h/%2Fa",
            "http://h/%2Fa",
            ["http", "h", "", "/%2Fa", "", ""],
            None,
        ),
        (
            "http://h/é",
            "http://h/%C3%A9",
            ["http", "h", "", "/%C3%A9", "", ""],
            None,
        ),
        (
            "http://h/a?b c",
            "http://h/a?b c",
            ["http", "h", "", "/a", "b c", ""],
            None,
        ),
        (
            "http://h/?a=1;b=2",
            "http://h/?a=1;b=2",
            ["http", "h", "", "/", "a=1;b=2", ""],
            None,
        ),
        (
            "http://h/?é",
            "http://h/?é",
            ["http", "h", "", "/", "é", ""],
            None,
        ),
        (
            "file:///etc/passwd",
            "file:///etc/passwd",
            ["file", "", "", "/etc/passwd", "", ""],
            None,
        ),
        (
            "file:/etc/passwd",
            "file:/etc/passwd",
            ["file", "", "", "/etc/passwd", "", ""],
            None,
        ),
        (
            "file://localhost/etc",
            "file://localhost/etc",
            ["file", "localhost", "", "/etc", "", ""],
            None,
        ),
        (
            "foo://h:99999999999/",
            "foo://h:99999999999/",
            ["foo", "h", "99999999999", "/", "", ""],
            None,
        ),
        (
            "http://h:0/",
            "http://h:0/",
            ["http", "h", "0", "/", "", ""],
            None,
        ),
        (
            "http://H:080/",
            "http://h:080/",
            ["http", "h", "080", "/", "", ""],
            None,
        ),
        (
            "http://xn--nxasmq6b.COM/",
            "http://xn--nxasmq6b.com/",
            ["http", "xn--nxasmq6b.com", "", "/", "", ""],
            None,
        ),
        ("HtTp://h", "http://h", ["http", "h", "", "", "", ""], None),
        (
            "a+b-c.d://h",
            "a+b-c.d://h",
            ["a+b-c.d", "h", "", "", "", ""],
            None,
        ),
        (
            "s3://bucket/key",
            "s3://bucket/key",
            ["s3", "bucket", "", "/key", "", ""],
            None,
        ),
        (
            "urn:isbn:0451450523",
            "urn:isbn:0451450523",
            ["urn", "", "", "", "", ""],
            None,
        ),
        (
            "tel:+1-816",
            "tel:+1-816",
            ["tel", "", "", "", "", ""],
            None,
        ),
        (
            "news:comp.infosystems.www.servers.unix",
            "news:comp.infosystems.www.servers.unix",
            ["news", "", "", "", "", ""],
            None,
        ),
        ("?q", "?q", ["", "", "", "", "q", ""], None),
        ("#f", "#f", ["", "", "", "", "", "f"], None),
        ("?", "?", ["", "", "", "", "", ""], None),
        ("#", "", ["", "", "", "", "", ""], None),
        ("a", "a", ["", "", "", "a", "", ""], None),
        ("a/b", "a/b", ["", "", "", "a/b", "", ""], None),
        ("/a/../b", "/a/../b", ["", "", "", "/a/../b", "", ""], None),
        ("//x/../y", "//x/../y", ["", "x", "", "/../y", "", ""], None),
        (
            "http://h/%41",
            "http://h/%41",
            ["http", "h", "", "/%41", "", ""],
            None,
        ),
        (
            "http://h/%e2%82%ac",
            "http://h/%e2%82%ac",
            ["http", "h", "", "/%e2%82%ac", "", ""],
            None,
        ),
        (
            "http://h/€",
            "http://h/%E2%82%AC",
            ["http", "h", "", "/%E2%82%AC", "", ""],
            None,
        ),
        (
            "http://h/a%2Fb c",
            "http://h/a/b%20c",
            ["http", "h", "", "/a/b%20c", "", ""],
            None,
        ),
        (
            "http://K/",
            "http://k/",
            ["http", "k", "", "/", "", ""],
            None,
        ),
        (
            "http://İ/",
            "http://i/",
            ["http", "i", "", "/", "", ""],
            None,
        ),
        (
            "http://Σ/",
            "http://%CF%83/",
            ["http", "σ", "", "/", "", ""],
            None,
        ),
        (
            "ftp://h:1:2",
            "ftp://h:1:2",
            ["ftp", "h:1", "2", "", "", ""],
            None,
        ),
        (
            "http://[::1%25x%2525]/",
            "http://[::1%25x%2525]/",
            ["http", "::1%x%25", "", "/", "", ""],
            None,
        ),
        (
            "http://[::1.2.3.4]/",
            "http://[::1.2.3.4]/",
            ["http", "::1.2.3.4", "", "/", "", ""],
            None,
        ),
        (
            "http://[1:2:3:4:5:6:7::]/",
            "http://[1:2:3:4:5:6:7::]/",
            ["http", "1:2:3:4:5:6:7::", "", "/", "", ""],
            None,
        ),
        (
            "http://[::]/",
            "http://[::]/",
            ["http", "::", "", "/", "", ""],
            None,
        ),
        (
            "http://[0:0:0:0:0:0:1.2.3.4]/",
            "http://[0:0:0:0:0:0:1.2.3.4]/",
            ["http", "0:0:0:0:0:0:1.2.3.4", "", "/", "", ""],
            None,
        ),
        (
            "http://h:/",
            "http://h:/",
            ["http", "h", "", "/", "", ""],
            None,
        ),
        ("http:h", "http:h", ["http", "", "", "", "", ""], None),
        ("http:h/p", "http:h/p", ["http", "", "", "", "", ""], None),
        ("HTTP:H", "http:H", ["http", "", "", "", "", ""], None),
        (
            "http:/h:p",
            "http:/h:p",
            ["http", "", "", "/h:p", "", ""],
            None,
        ),
        ("x:/a//b", "x:/a//b", ["x", "", "", "/a//b", "", ""], None),
        ("x:%2F%2Fa", "x:%2F%2Fa", ["x", "", "", "", "", ""], None),
        (
            "x:/%2F%2Fa",
            "x:/%2F%2Fa",
            ["x", "", "", "/%2F%2Fa", "", ""],
            None,
        ),
        (
            "x:/%2F%2Fa b",
            "x:%2F//a%20b",
            ["x", "", "", "///a%20b", "", ""],
            None,
        ),
        ("x:///%2F", "x:///%2F", ["x", "", "", "/%2F", "", ""], None),
        ("//h:80", "//h:80", ["", "h", "80", "", "", ""], None),
        ("//u@h", "//u@h", ["", "h", "", "", "", ""], Some("u")),
        ("///a:b", "///a:b", ["", "", "", "///a:b", "", ""], None),
        ("a b", "a%20b", ["", "", "", "a%20b", "", ""], None),
        ("?%zz", "?%zz", ["", "", "", "", "%zz", ""], None),
        (
            "http://h/a\\b",
            "http://h/a%5Cb",
            ["http", "h", "", "/a%5Cb", "", ""],
            None,
        ),
        (
            "http://h/{}|^`",
            "http://h/%7B%7D%7C%5E%60",
            ["http", "h", "", "/%7B%7D%7C%5E%60", "", ""],
            None,
        ),
        (
            "http://h?{}|^`",
            "http://h?{}|^`",
            ["http", "h", "", "", "{}|^`", ""],
            None,
        ),
        (
            "http://h#{}|^`\"<>",
            "http://h#%7B%7D%7C%5E%60%22%3C%3E",
            ["http", "h", "", "", "", "{}|^`\"<>"],
            None,
        ),
    ];

    /// Inputs Go 1.27.1 `url.Parse` rejects.
    const REJECT: &[&str] = &[
        ":",
        ":foo",
        "1http://x",
        "http://h:8a",
        "http://h:1:2",
        "http://::1/",
        "http://[::1]x",
        "http://a[::1]",
        "http://[1.2.3.4]/",
        "http://[fe80::1%25]/",
        "http://[fe80::1%25%00]/",
        "http://[fe80::1%en0]/",
        "http://[::1",
        "http://]::1[",
        "http://%41/",
        "http://h%20/",
        "http://h^/",
        "http://é@h",
        "http://u%zz@h",
        "http://h#%zz",
        "http://h\u{9}",
        "http://h/\u{7f}",
        "http://h/%",
        "http://h/%4",
        "http://h:80:80",
        "https://h:1:2",
        "http://[::1]:80:80",
        "http://[::1]:a",
        "http://[::1]]",
        "http://[[::1]]",
        "http://[1::2::3]/",
        "http://[1:2:3:4:5:6:7:8:9]/",
        "http://[0:0:0:0:0:1.2.3.4]/",
        "http://[1:2:3:4:5:6:7:1.2.3.4]/",
        "http://[::ffff:01.2.3.4]/",
        "http://[12345::]/",
        "http://[%31::]/",
        "http://[::%c3%a9]/",
        "a b:c",
        "%zz",
        "/%zz",
        "#%zz",
    ];

    #[test]
    fn parse_matches_go() {
        for &(input, text, [scheme, host, port, path, query, fragment], user) in ACCEPT {
            let u = Url::parse(input).unwrap_or_else(|e| panic!("parse({input:?}): {e}"));
            assert_eq!(u.as_str(), text, "text of {input:?}");
            assert_eq!(u.scheme(), scheme, "scheme of {input:?}");
            assert_eq!(u.host(), host, "host of {input:?}");
            assert_eq!(u.port(), port, "port of {input:?}");
            assert_eq!(u.escaped_path(), path, "path of {input:?}");
            assert_eq!(u.raw_query(), query, "query of {input:?}");
            assert_eq!(u.fragment(), fragment, "fragment of {input:?}");
            assert_eq!(u.username(), user, "user of {input:?}");
        }
        for &input in REJECT {
            assert!(Url::parse(input).is_err(), "parse({input:?}) should fail");
        }
    }

    /// The URL rows of testdata/vectors/text_forms.json.
    #[test]
    fn text_forms_vectors() {
        for (input, text) in [
            (
                "HTTPS://Example.COM:443/a%20b/../c?q=A+b&x=%41#Frag%20x",
                "https://example.com:443/a%20b/../c?q=A+b&x=%41#Frag%20x",
            ),
            ("http://h", "http://h"),
            ("http://h?", "http://h?"),
            ("http://[::1]:80/p", "http://[::1]:80/p"),
            ("http://user:pw@h", "http://user:pw@h"),
            ("mailto:a@b.com", "mailto:a@b.com"),
            ("http://h/a b", "http://h/a%20b"),
        ] {
            let u = Url::parse(input).unwrap();
            assert_eq!(u.as_str(), text, "{input:?}");
            assert_eq!(u.to_string(), text, "{input:?}");
        }
    }

    /// The URL of testdata/vectors/fields.json.
    #[test]
    fn fields_vector() {
        let u =
            Url::parse("https://alice@Example.com:8443/api/v1?tag=a&tag=b&env=prod#top").unwrap();
        assert_eq!(u.scheme(), "https");
        assert_eq!(u.host(), "example.com");
        assert_eq!(u.port(), "8443");
        assert_eq!(u.escaped_path(), "/api/v1");
        assert_eq!(u.raw_query(), "tag=a&tag=b&env=prod");
        assert_eq!(u.fragment(), "top");
        assert_eq!(u.username(), Some("alice"));
        let bare = Url::parse("http://example.com").unwrap();
        assert_eq!(bare.username(), None);
        assert_eq!(bare.port(), "");
        assert_eq!(bare.escaped_path(), "");
        assert_eq!(
            Url::parse("https://h/a%20b/c%2Fd").unwrap().escaped_path(),
            "/a%20b/c%2Fd"
        );
    }

    #[test]
    fn equal_urls_are_equal_values() {
        assert_eq!(Url::parse("HTTP://H/p"), Url::parse("http://h/p"));
        assert_ne!(Url::parse("http://h/p"), Url::parse("http://h/q"));
    }
}
