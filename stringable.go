package rulekit

import (
	"net"
	"net/url"
)

// stringable returns the text form of a value that compares with strings:
// strings themselves and values with a canonical text form (IP addresses,
// CIDRs, MAC addresses, URLs, and URL queries). Other values, including
// numbers and booleans, are not stringable.
func stringable(value any) (string, bool) {
	switch v := value.(type) {
	case string:
		return v, true
	case urlQuery:
		return string(v), true
	case net.IP:
		return v.String(), true
	case *net.IPNet:
		return v.String(), true
	case net.HardwareAddr:
		return v.String(), true
	case *url.URL:
		return v.String(), true
	}
	return "", false
}
