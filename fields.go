package rulekit

import (
	"net"
	"net/url"
	"strconv"
	"strings"
)

// valueField resolves a built-in field of a typed value, such as url.host or
// ip.version. It reports false when the value has no such field or the field
// is absent from the value.
func valueField(value any, key string) (any, bool) {
	switch v := value.(type) {
	case *url.URL:
		return urlField(v, key)
	case urlQuery:
		return queryField(string(v), key)
	case net.IP:
		if key == "version" {
			return ipVersion(v)
		}
	case *net.IPNet:
		return cidrField(v, key)
	case net.HardwareAddr:
		if key == "oui" && len(v) >= 3 {
			return v[:3], true
		}
	}
	return nil, false
}

func urlField(u *url.URL, key string) (any, bool) {
	if u == nil {
		return nil, false
	}
	switch key {
	case "scheme":
		return nonEmpty(u.Scheme)
	case "host":
		return nonEmpty(strings.ToLower(u.Hostname()))
	case "port":
		port, err := strconv.Atoi(u.Port())
		if err != nil {
			return nil, false
		}
		return port, true
	case "path":
		return u.EscapedPath(), true
	case "query":
		return urlQuery(u.RawQuery), true
	case "fragment":
		return nonEmpty(u.Fragment)
	case "user":
		if u.User == nil {
			return nil, false
		}
		return nonEmpty(u.User.Username())
	}
	return nil, false
}

// urlQuery is a URL's raw query string. Parameters are decoded only when a
// rule asks for one by name; as a whole it compares as its text.
type urlQuery string

// queryField finds a query parameter by name using the WHATWG
// application/x-www-form-urlencoded rules: pairs are separated by "&", the
// first "=" splits name from value, "+" is a space, and percent escapes are
// decoded (invalid escapes are kept as written). A single value is a string
// and repeated values are a list.
// https://url.spec.whatwg.org/#concept-urlencoded-parser
func queryField(raw, key string) (any, bool) {
	var (
		first  string
		values []string
		found  bool
	)
	for raw != "" {
		var pair string
		pair, raw, _ = strings.Cut(raw, "&")
		if pair == "" {
			continue
		}
		name, value, _ := strings.Cut(pair, "=")
		if formDecode(name) != key {
			continue
		}
		value = formDecode(value)
		switch {
		case !found:
			first, found = value, true
		case values == nil:
			values = []string{first, value}
		default:
			values = append(values, value)
		}
	}
	switch {
	case values != nil:
		return values, true
	case found:
		return first, true
	}
	return nil, false
}

// formDecode decodes "+" and percent escapes in a urlencoded name or value,
// keeping invalid escapes as written. Invalid UTF-8 becomes U+FFFD.
func formDecode(s string) string {
	if !strings.ContainsAny(s, "+%") {
		return s
	}
	b := make([]byte, 0, len(s))
	for i := 0; i < len(s); i++ {
		switch c := s[i]; {
		case c == '+':
			b = append(b, ' ')
		case c == '%' && i+2 < len(s) && isHexDigit(s[i+1]) && isHexDigit(s[i+2]):
			b = append(b, unhex(s[i+1])<<4|unhex(s[i+2]))
			i += 2
		default:
			b = append(b, c)
		}
	}
	return strings.ToValidUTF8(string(b), "\uFFFD")
}

func isHexDigit(c byte) bool {
	return '0' <= c && c <= '9' || 'a' <= c && c <= 'f' || 'A' <= c && c <= 'F'
}

func unhex(c byte) byte {
	switch {
	case c <= '9':
		return c - '0'
	case c <= 'F':
		return c - 'A' + 10
	default:
		return c - 'a' + 10
	}
}

func cidrField(n *net.IPNet, key string) (any, bool) {
	if n == nil {
		return nil, false
	}
	switch key {
	case "network":
		return n.IP, true
	case "prefix":
		ones, bits := n.Mask.Size()
		if bits == 0 {
			return nil, false
		}
		// An IPv4-mapped network is an IPv4 network: ::ffff:10.0.0.0/104 is /8.
		if bits == 8*net.IPv6len && n.IP.To4() != nil {
			ones -= 96
		}
		return ones, true
	case "version":
		return ipVersion(n.IP)
	}
	return nil, false
}

func ipVersion(ip net.IP) (any, bool) {
	if ip.To4() != nil {
		return "v4", true
	}
	if len(ip) == net.IPv6len {
		return "v6", true
	}
	return nil, false
}

func nonEmpty(s string) (any, bool) {
	if s == "" {
		return nil, false
	}
	return s, true
}
