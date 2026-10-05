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
	case url.Values:
		return queryField(v, key)
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
		return u.Path, true
	case "query":
		return u.Query(), true
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

// queryField returns a single value as a string and repeated values as a list.
func queryField(values url.Values, key string) (any, bool) {
	switch v := values[key]; len(v) {
	case 0:
		return nil, false
	case 1:
		return v[0], true
	default:
		return v, true
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
