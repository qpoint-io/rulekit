package rulekit

import (
	"net"
	"testing"
)

func TestQuotedLiteralTypes(t *testing.T) {
	ip := net.ParseIP("192.168.1.1")
	mac := mustParseMac("01:23:45:67:89:ab")
	input := kv{"s": "just a string", "ip": ip, "mac": mac}

	// A plain quoted value is a string.
	assertRulep(t, `s == "just a string"`, input).Ok().DoesPass(true)
	// Quoted IPs compare as IPs, not text: forms that differ as text are equal.
	assertRulep(t, `ip == "192.168.1.1"`, input).Ok().DoesPass(true)
	assertRulep(t, `"2001:db8::1" == 2001:db8:0:0:0:0:0:1`, nil).Ok().DoesPass(true)
	assertRulep(t, `"10.1.2.3" in 10.0.0.0/8`, nil).Ok().DoesPass(true)
	// A quoted CIDR is a CIDR.
	assertRulep(t, `ip in "192.168.1.0/24"`, input).Ok().DoesPass(true)
	// Quoted MACs are MACs in any standard notation.
	assertRulep(t, `mac == "01:23:45:67:89:ab"`, input).Ok().DoesPass(true)
	assertRulep(t, `mac == "01-23-45-67-89-AB"`, input).Ok().DoesPass(true)
	assertRulep(t, `mac == "0123.4567.89ab"`, input).Ok().DoesPass(true)
}

func mustParseMac(s string) net.HardwareAddr {
	mac, err := net.ParseMAC(s)
	if err != nil {
		panic(err)
	}
	return mac
}
