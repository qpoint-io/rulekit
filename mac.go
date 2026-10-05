package rulekit

import (
	"encoding/hex"
	"net"
	"strings"
)

// parseMAC parses a 6- or 8-byte MAC address written as hex pairs separated
// by colons or hyphens (01:23:45:67:89:ab, 01-23-45-67-89-ab) or as groups
// of four hex digits separated by dots (0123.4567.89ab). Other notations are
// not MAC addresses.
func parseMAC(s string) (net.HardwareAddr, bool) {
	var groups []string
	var width int
	switch {
	case strings.Contains(s, ":"):
		groups, width = strings.Split(s, ":"), 2
	case strings.Contains(s, "-"):
		groups, width = strings.Split(s, "-"), 2
	case strings.Contains(s, "."):
		groups, width = strings.Split(s, "."), 4
	default:
		return nil, false
	}
	if size := len(groups) * width / 2; size != 6 && size != 8 {
		return nil, false
	}
	mac := make(net.HardwareAddr, 0, len(groups)*width/2)
	for _, group := range groups {
		if len(group) != width {
			return nil, false
		}
		b, err := hex.DecodeString(group)
		if err != nil {
			return nil, false
		}
		mac = append(mac, b...)
	}
	return mac, true
}
