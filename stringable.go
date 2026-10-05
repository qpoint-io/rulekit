package rulekit

import (
	"net"
	"net/url"
	"strings"
)

// stringable returns the text form of a value that compares with strings:
// strings themselves and values with a canonical text form (IP addresses,
// CIDRs, MAC addresses, URLs, and URL queries). Other values, including
// numbers and booleans, are not stringable. The text forms are part of the
// language; see testdata/vectors/README.md.
func stringable(value any) (string, bool) {
	switch v := value.(type) {
	case string:
		return v, true
	case urlQuery:
		return string(v), true
	case net.IP:
		return ipText(v), true
	case *net.IPNet:
		return cidrText(v), true
	case net.HardwareAddr:
		return macText(v), true
	case *url.URL:
		return urlText(v), true
	}
	return "", false
}

// ipText is dotted decimal for IPv4 and IPv4-mapped IPv6 addresses, and
// RFC 5952 text (lowercase, longest zero run compressed) for other IPv6.
func ipText(ip net.IP) string {
	return ip.String()
}

// cidrText is the network address (host bits cleared) in ipText form,
// followed by "/" and the prefix length.
func cidrText(n *net.IPNet) string {
	return n.String()
}

// macText is lowercase hex pairs separated by colons.
func macText(mac net.HardwareAddr) string {
	return mac.String()
}

// urlText is the URL with its scheme and host lowercased and every other part
// as written. Characters that are not allowed in a URL are percent-encoded.
func urlText(u *url.URL) string {
	lower := *u
	lower.Host = strings.ToLower(u.Host)
	return lower.String()
}
