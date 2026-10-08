package rulekit

import (
	"net"
	"strings"
)

// parseMAC parses a 6- or 8-byte MAC address written as hex pairs separated
// by colons or hyphens (01:23:45:67:89:ab, 01-23-45-67-89-ab) or as groups
// of four hex digits separated by dots (0123.4567.89ab). Other notations are
// not MAC addresses.
func parseMAC(s string) (net.HardwareAddr, bool) {
	if !strings.ContainsAny(s, ":-.") {
		return nil, false
	}
	mac, err := net.ParseMAC(s)
	if err != nil || (len(mac) != 6 && len(mac) != 8) {
		return nil, false
	}
	return mac, true
}
