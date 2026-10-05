//! URLs: RFC 3986 URI references, parsed by `fluent-uri` without
//! normalization. The text form is the URL as written with the scheme and
//! host lowercased.

use std::borrow::Cow;
use std::fmt;

use fluent_uri::UriRef;
use fluent_uri::pct_enc::{EStr, Encoder};

/// A parsed URL. Every accessor borrows from data computed once by
/// [`Url::parse`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Url {
    /// The text form, followed by the decoded parts that are not substrings
    /// of it.
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
    /// Parses a URL by the shared rules: an RFC 3986 URI reference (absolute
    /// or relative) in ASCII, with no `%` escapes in the host and no IPvFuture
    /// literal. Anything else is an error.
    pub fn parse(s: &str) -> Result<Url, String> {
        let r = UriRef::parse(s).map_err(|e| format!("invalid URL {s:?}: {e}"))?;
        let at = |part: &str| Span {
            start: part.as_ptr() as usize - s.as_ptr() as usize,
            end: part.as_ptr() as usize - s.as_ptr() as usize + part.len(),
        };

        // The input is ASCII, so lowercasing keeps every offset.
        let mut buf = String::from(s);
        let scheme = r.scheme().map_or(Span::EMPTY, |sc| at(sc.as_str()));
        buf[scheme.start..scheme.end].make_ascii_lowercase();

        let (mut host, mut port, mut user) = (Span::EMPTY, Span::EMPTY, None);
        if let Some(auth) = r.authority() {
            let h = at(auth.host());
            if auth.host().contains('%') {
                return Err(format!("invalid URL {s:?}: percent escape in host"));
            }
            if auth.host().starts_with("[v") || auth.host().starts_with("[V") {
                return Err(format!("invalid URL {s:?}: IPvFuture host"));
            }
            buf[h.start..h.end].make_ascii_lowercase();
            host = if auth.host().starts_with('[') {
                Span {
                    start: h.start + 1,
                    end: h.end - 1,
                }
            } else {
                h
            };
            port = auth.port().map_or(Span::EMPTY, |p| at(p.as_str()));
            user = auth.userinfo().map(|info| {
                let name = info.split_once(':').map_or(info, |(name, _)| name);
                decoded(&mut buf, name, at(name.as_str()))
            });
        }

        let path = at(r.path().as_str());
        let query = r.query().map_or(Span::EMPTY, |q| at(q.as_str()));
        let fragment = r
            .fragment()
            .map_or(Span::EMPTY, |f| decoded(&mut buf, f, at(f.as_str())));
        Ok(Url {
            buf: buf.into_boxed_str(),
            text_end: s.len(),
            scheme,
            host,
            port,
            path,
            query,
            fragment,
            user,
        })
    }

    /// The text form: the URL as written with the scheme and host lowercase.
    pub fn as_str(&self) -> &str {
        &self.buf[..self.text_end]
    }

    /// The scheme, lowercase; empty for a relative URL.
    pub fn scheme(&self) -> &str {
        self.get(self.scheme)
    }

    /// The host, lowercase, without port; IPv6 brackets are stripped. Empty
    /// without an authority.
    pub fn host(&self) -> &str {
        self.get(self.host)
    }

    /// The port digits as written, or empty.
    pub fn port(&self) -> &str {
        self.get(self.port)
    }

    /// The path as written.
    pub fn escaped_path(&self) -> &str {
        self.get(self.path)
    }

    /// The query as written, without the `?`; empty if none.
    pub fn raw_query(&self) -> &str {
        self.get(self.query)
    }

    /// The fragment, percent-decoded; as written if the decoded bytes are not
    /// UTF-8. Empty if none.
    pub fn fragment(&self) -> &str {
        self.get(self.fragment)
    }

    /// The user name (user info before the first `:`), percent-decoded; as
    /// written if the decoded bytes are not UTF-8. `None` without user info.
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

/// The span of `part` (at `raw` in the text) percent-decoded: `raw` itself
/// when decoding changes nothing or yields bytes that are not UTF-8,
/// otherwise the decoded text appended to `buf` after the text form.
fn decoded<E: Encoder>(buf: &mut String, part: &EStr<E>, raw: Span) -> Span {
    match part.decode().to_string() {
        Ok(Cow::Owned(text)) => {
            let start = buf.len();
            buf.push_str(&text);
            Span {
                start,
                end: buf.len(),
            }
        }
        Ok(Cow::Borrowed(_)) | Err(_) => raw,
    }
}

#[cfg(test)]
mod tests {
    use super::Url;

    /// (input, text form, [scheme, host, port, escaped path, raw query,
    /// fragment], username).
    type Row = (
        &'static str,
        &'static str,
        [&'static str; 6],
        Option<&'static str>,
    );

    /// Results that Go 1.27.1 `net/url` agrees with.
    const ACCEPT: &[Row] = &[
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
        (
            "http://h#a?/:@",
            "http://h#a?/:@",
            ["http", "h", "", "", "", "a?/:@"],
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
            "http://[2001:DB8::A]/",
            "http://[2001:db8::a]/",
            ["http", "2001:db8::a", "", "/", "", ""],
            None,
        ),
        (
            "http://h:/",
            "http://h:/",
            ["http", "h", "", "/", "", ""],
            None,
        ),
        (
            "http://h:99999999/",
            "http://h:99999999/",
            ["http", "h", "99999999", "/", "", ""],
            None,
        ),
        (
            "http://user:pw@h",
            "http://user:pw@h",
            ["http", "h", "", "", "", ""],
            Some("user"),
        ),
        (
            "http://@h",
            "http://@h",
            ["http", "h", "", "", "", ""],
            Some(""),
        ),
        (
            "http://:pw@h",
            "http://:pw@h",
            ["http", "h", "", "", "", ""],
            Some(""),
        ),
        (
            "http://A%20B:x%3A@h",
            "http://A%20B:x%3A@h",
            ["http", "h", "", "", "", ""],
            Some("A B"),
        ),
        (
            "http://h#%C3%A9",
            "http://h#%C3%A9",
            ["http", "h", "", "", "", "é"],
            None,
        ),
        (
            "http://h#a+b",
            "http://h#a+b",
            ["http", "h", "", "", "", "a+b"],
            None,
        ),
        (
            "file:///p",
            "file:///p",
            ["file", "", "", "/p", "", ""],
            None,
        ),
        (
            "http:///p",
            "http:///p",
            ["http", "", "", "/p", "", ""],
            None,
        ),
        ("//H/p", "//h/p", ["", "h", "", "/p", "", ""], None),
        ("/p?q#f", "/p?q#f", ["", "", "", "/p", "q", "f"], None),
        ("a/b", "a/b", ["", "", "", "a/b", "", ""], None),
        ("./a:b", "./a:b", ["", "", "", "./a:b", "", ""], None),
        ("http:", "http:", ["http", "", "", "", "", ""], None),
        (
            "http://h/a%2Fb/%2f/../.",
            "http://h/a%2Fb/%2f/../.",
            ["http", "h", "", "/a%2Fb/%2f/../.", "", ""],
            None,
        ),
        (
            "http://h/!$&'()*+,;=:@~",
            "http://h/!$&'()*+,;=:@~",
            ["http", "h", "", "/!$&'()*+,;=:@~", "", ""],
            None,
        ),
        (
            "http://h?/?:@",
            "http://h?/?:@",
            ["http", "h", "", "", "/?:@", ""],
            None,
        ),
    ];

    /// Accepted by the shared rules, where Go's `net/url` gives a different
    /// text form or field (noted per row); rulekit's Go `URL` follows the
    /// shared rules.
    const ACCEPT_UNLIKE_GO: &[Row] = &[
        // Go drops an empty fragment from the text form: "http://h".
        (
            "http://h#",
            "http://h#",
            ["http", "h", "", "", "", ""],
            None,
        ),
        (
            "http://h?#",
            "http://h?#",
            ["http", "h", "", "", "", ""],
            None,
        ),
        // Go drops an empty authority when the path is empty: "http:".
        ("http://", "http://", ["http", "", "", "", "", ""], None),
        // Go reads a non-UTF-8 decoded fragment or user name as bytes.
        (
            "http://%ff@h",
            "http://%ff@h",
            ["http", "h", "", "", "", ""],
            Some("%ff"),
        ),
        (
            "http://h#%ff",
            "http://h#%ff",
            ["http", "h", "", "", "", "%ff"],
            None,
        ),
        // Go re-encodes user info: "http://UA:p%3Aq@h".
        (
            "http://U%41:p:q@h",
            "http://U%41:p:q@h",
            ["http", "h", "", "", "", ""],
            Some("UA"),
        ),
        // Go: the path of an opaque URL is empty.
        (
            "mailto:a@b.com",
            "mailto:a@b.com",
            ["mailto", "", "", "a@b.com", "", ""],
            None,
        ),
        (
            "Mailto:A@B",
            "mailto:A@B",
            ["mailto", "", "", "A@B", "", ""],
            None,
        ),
        ("http:p", "http:p", ["http", "", "", "p", "", ""], None),
        // Go: no authority, path "///p".
        ("///p", "///p", ["", "", "", "/p", "", ""], None),
    ];

    /// Inputs that are not RFC 3986 URI references and that Go 1.27.1
    /// `net/url` rejects too.
    const REJECT: &[&str] = &[
        "http://h/%",
        "http://h/%4",
        "http://h/%zz",
        "http://h#%zz",
        "http://h/\n",
        "http://h?\u{7f}",
        "http://h:8x/",
        "http://h:80:80/",
        "http://[::1/",
        "http://[::1]x/",
        "http://[fe80::1%en0]/",
        "http://[1.2.3.4]/",
        "http://[:::1]/",
        "http://h{/",
        ":",
        ":foo",
        "1http://h",
        "%",
        " http://h",
    ];

    /// Inputs the shared rules reject but Go's `net/url` accepts (rulekit's
    /// Go `URL` rejects them too).
    const REJECT_UNLIKE_GO: &[&str] = &[
        // An IPvFuture literal or a percent escape in the host.
        "http://[v1.Ab:C]/",
        "http://Ex%41mple.COM/",
        "http://h%C3%A9/",
        // Characters outside RFC 3986 in the path, query, or fragment.
        "http://h/a b",
        "http://h/é",
        "http://h/\"",
        "http://h/<>",
        "http://h/{}",
        "http://h/|",
        "http://h/\\",
        "http://h/^",
        "http://h/`",
        "http://h/[",
        "a:b c",
        "http://h?a b",
        "http://h?é",
        "http://h?[]",
        "http://h#a b",
        "http://h#é",
        "http://h#\u{1}",
        "http://h#a#b",
        "http://h#a#",
        // Invalid percent escapes where Go does not decode.
        "http://h/?c=%zz",
        "mailto:a%zz",
        // Hosts: non-ASCII, `"<>`, IPv6 zones, more than one colon.
        "http://é/",
        "http://h\"/",
        "http://[fe80::1%25en0]/",
        "//h:80:80",
        "ws://h:80:80/",
        // A second `@` in the authority.
        "http://a@b@c/",
    ];

    fn check_accepts(rows: &[Row]) {
        for (input, text, fields, user) in rows {
            let u = Url::parse(input).unwrap_or_else(|e| panic!("{input:?}: {e}"));
            let got = [
                u.scheme(),
                u.host(),
                u.port(),
                u.escaped_path(),
                u.raw_query(),
                u.fragment(),
            ];
            assert_eq!(
                (u.as_str(), got, u.username()),
                (*text, *fields, *user),
                "{input:?}"
            );
            assert_eq!(u.to_string(), *text);
        }
    }

    #[test]
    fn parse_accepts() {
        check_accepts(ACCEPT);
        check_accepts(ACCEPT_UNLIKE_GO);
    }

    #[test]
    fn parse_rejects() {
        for input in REJECT.iter().chain(REJECT_UNLIKE_GO) {
            assert!(Url::parse(input).is_err(), "{input:?} accepted");
        }
    }

    #[test]
    fn text_forms_vectors() {
        // The rows of testdata/vectors/text_forms.json that are URI references.
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
        ] {
            assert_eq!(Url::parse(input).unwrap().as_str(), text);
        }
    }

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
    }

    #[test]
    fn equal_urls_are_equal_values() {
        assert_eq!(
            Url::parse("HTTP://H/p").unwrap(),
            Url::parse("http://h/p").unwrap()
        );
        assert_ne!(
            Url::parse("http://h/p").unwrap(),
            Url::parse("http://h/P").unwrap()
        );
    }
}
