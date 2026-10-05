package rulekit

import (
	"fmt"
	"net/netip"
	"strings"
	"unicode/utf8"
)

// URL is a URL value that follows rulekit's shared URL rules, the same in
// every rulekit implementation. `$url` inputs decode to URL; build one with
// ParseURL. (A *url.URL placed in a KV also works, but compares by its
// String() form with the host lowercased.)
//
// A URL is an RFC 3986 URI reference written in ASCII, with no `%` escapes
// in the host, no IPvFuture literal, and no IPv6 zone. Its text form is the
// input as written with the scheme and host lowercased.
type URL struct {
	text     string
	scheme   string
	user     string
	hasUser  bool
	host     string
	port     string
	path     string
	query    string
	fragment string
}

// ParseURL parses s by the shared URL rules.
func ParseURL(s string) (URL, error) {
	u, err := parseURL(s)
	if err != nil {
		return URL{}, fmt.Errorf("invalid URL %q: %w", s, err)
	}
	return u, nil
}

// String returns the text form: the URL as written with the scheme and host
// lowercased.
func (u URL) String() string { return u.text }

func parseURL(s string) (URL, error) {
	for i := range len(s) {
		if s[i] >= utf8.RuneSelf {
			return URL{}, fmt.Errorf("non-ASCII character")
		}
	}

	// Split into components (RFC 3986 appendix B).
	var u URL
	rest := s
	schemeEnd := strings.IndexAny(rest, ":/?#")
	if schemeEnd > 0 && rest[schemeEnd] == ':' && validScheme(rest[:schemeEnd]) {
		u.scheme = strings.ToLower(rest[:schemeEnd])
		rest = rest[schemeEnd+1:]
	}
	hasFragment, hasQuery := false, false
	if i := strings.IndexByte(rest, '#'); i >= 0 {
		u.fragment, hasFragment = rest[i+1:], true
		rest = rest[:i]
	}
	if i := strings.IndexByte(rest, '?'); i >= 0 {
		u.query, hasQuery = rest[i+1:], true
		rest = rest[:i]
	}
	authority, hasAuthority := "", false
	if strings.HasPrefix(rest, "//") {
		rest = rest[2:]
		end := strings.IndexByte(rest, '/')
		if end < 0 {
			end = len(rest)
		}
		authority, rest, hasAuthority = rest[:end], rest[end:], true
	}
	u.path = rest

	// Validate components.
	hostText, afterHost := "", ""
	if hasAuthority {
		var err error
		if hostText, afterHost, err = u.parseAuthority(authority); err != nil {
			return URL{}, err
		}
	}
	if !validChars(u.path, "/:@") {
		return URL{}, fmt.Errorf("invalid character in path")
	}
	if u.scheme == "" && !hasAuthority {
		// A relative path's first segment may not contain ':'.
		first, _, _ := strings.Cut(u.path, "/")
		if strings.Contains(first, ":") {
			return URL{}, fmt.Errorf("first path segment of a relative URL contains ':'")
		}
	}
	if !validChars(u.query, "/:@?") {
		return URL{}, fmt.Errorf("invalid character in query")
	}
	if !validChars(u.fragment, "/:@?") {
		return URL{}, fmt.Errorf("invalid character in fragment")
	}

	// Text form: as written, with scheme and host lowercased.
	var b strings.Builder
	b.Grow(len(s))
	if u.scheme != "" {
		b.WriteString(u.scheme)
		b.WriteByte(':')
	}
	if hasAuthority {
		b.WriteString("//")
		if at := strings.LastIndexByte(authority, '@'); at >= 0 {
			b.WriteString(authority[:at+1])
		}
		b.WriteString(strings.ToLower(hostText))
		b.WriteString(afterHost)
	}
	b.WriteString(u.path)
	if hasQuery {
		b.WriteByte('?')
		b.WriteString(u.query)
	}
	if hasFragment {
		b.WriteByte('#')
		b.WriteString(u.fragment)
	}
	u.text = b.String()
	u.fragment = decodeOrKeep(u.fragment)
	return u, nil
}

// parseAuthority validates `[userinfo@]host[:port]`, sets the user, host, and
// port fields, and returns the host as written and the `:port` after it.
func (u *URL) parseAuthority(authority string) (host, afterHost string, err error) {
	hostport := authority
	if at := strings.IndexByte(authority, '@'); at >= 0 {
		userinfo := authority[:at]
		hostport = authority[at+1:]
		if strings.Contains(hostport, "@") {
			return "", "", fmt.Errorf("more than one '@'")
		}
		if !validChars(userinfo, ":") {
			return "", "", fmt.Errorf("invalid character in userinfo")
		}
		name, _, _ := strings.Cut(userinfo, ":")
		u.user, u.hasUser = decodeOrKeep(name), true
	}

	host = hostport
	if strings.HasPrefix(hostport, "[") {
		end := strings.IndexByte(hostport, ']')
		if end < 0 {
			return "", "", fmt.Errorf("missing ']' in host")
		}
		host = hostport[:end+1]
		addr, err := netip.ParseAddr(host[1:end])
		if err != nil || !addr.Is6() || addr.Zone() != "" {
			return "", "", fmt.Errorf("invalid IPv6 literal")
		}
		u.host = strings.ToLower(host[1:end])
	} else {
		if i := strings.IndexByte(hostport, ':'); i >= 0 {
			host = hostport[:i]
		}
		if strings.Contains(host, "%") || !validChars(host, "") {
			return "", "", fmt.Errorf("invalid character in host")
		}
		u.host = strings.ToLower(host)
	}
	afterHost = hostport[len(host):]
	if afterHost != "" {
		port, ok := strings.CutPrefix(afterHost, ":")
		if !ok || strings.Trim(port, "0123456789") != "" {
			return "", "", fmt.Errorf("invalid port")
		}
		u.port = port
	}
	return host, afterHost, nil
}

func validScheme(s string) bool {
	for i := range len(s) {
		c := s[i]
		switch {
		case 'a' <= c && c <= 'z' || 'A' <= c && c <= 'Z':
		case i > 0 && ('0' <= c && c <= '9' || c == '+' || c == '-' || c == '.'):
		default:
			return false
		}
	}
	return s != ""
}

// validChars reports whether s has only RFC 3986 unreserved characters,
// sub-delims, valid percent escapes, and the extra characters in extra.
func validChars(s, extra string) bool {
	for i := 0; i < len(s); i++ {
		c := s[i]
		switch {
		case 'a' <= c && c <= 'z', 'A' <= c && c <= 'Z', '0' <= c && c <= '9':
		case strings.IndexByte("-._~!$&'()*+,;=", c) >= 0, strings.IndexByte(extra, c) >= 0:
		case c == '%' && i+2 < len(s) && isHexDigit(s[i+1]) && isHexDigit(s[i+2]):
			i += 2
		default:
			return false
		}
	}
	return true
}

// decodeOrKeep percent-decodes s; if the result is not valid UTF-8, s is
// kept as written.
func decodeOrKeep(s string) string {
	if !strings.Contains(s, "%") {
		return s
	}
	b := make([]byte, 0, len(s))
	for i := 0; i < len(s); i++ {
		if s[i] == '%' {
			b = append(b, unhex(s[i+1])<<4|unhex(s[i+2]))
			i += 2
			continue
		}
		b = append(b, s[i])
	}
	if !utf8.Valid(b) {
		return s
	}
	return string(b)
}

// urlValueField resolves a field of a URL value.
func urlValueField(u URL, key string) (any, bool) {
	switch key {
	case "scheme":
		return nonEmpty(u.scheme)
	case "host":
		return nonEmpty(u.host)
	case "port":
		return portNumber(u.port)
	case "path":
		return u.path, true
	case "query":
		return urlQuery(u.query), true
	case "fragment":
		return nonEmpty(u.fragment)
	case "user":
		if !u.hasUser {
			return nil, false
		}
		return nonEmpty(u.user)
	}
	return nil, false
}
