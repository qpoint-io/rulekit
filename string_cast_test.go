package rulekit

import (
	"net"
	"reflect"
	"testing"

	"github.com/stretchr/testify/assert"
)

func TestStringAutomaticCasting(t *testing.T) {
	tests := []struct {
		name      string
		ruleExpr  string
		inputData map[string]any
		expected  bool
	}{
		{
			name:      "explicit IP equals string IP",
			ruleExpr:  `ip == "192.168.1.1"`,
			inputData: map[string]any{"ip": net.ParseIP("192.168.1.1")},
			expected:  true,
		},
		{
			name:      "string IP equals explicit IP",
			ruleExpr:  `"192.168.1.1" == ip`,
			inputData: map[string]any{"ip": net.ParseIP("192.168.1.1")},
			expected:  true,
		},
		{
			name:      "string field with IP value equals explicit IP",
			ruleExpr:  `ipstr == 192.168.1.1`,
			inputData: map[string]any{"ipstr": "192.168.1.1"},
			expected:  true,
		},
		{
			name:      "string IP in CIDR array",
			ruleExpr:  `"192.168.1.5" in [192.168.1.0/24]`,
			inputData: map[string]any{},
			expected:  true,
		},
		{
			name:      "string IP not in CIDR array",
			ruleExpr:  `"10.0.0.1" in [192.168.1.0/24]`,
			inputData: map[string]any{},
			expected:  false,
		},
		{
			name:      "string MAC equals MAC",
			ruleExpr:  `mac == "01:23:45:67:89:ab"`,
			inputData: map[string]any{"mac": mustParseMac("01:23:45:67:89:ab")},
			expected:  true,
		},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			assertRulep(t, tc.ruleExpr, kv(tc.inputData)).
				Ok().
				DoesPass(tc.expected)
		})
	}
}

func TestTryParseAs(t *testing.T) {
	tests := []struct {
		name     string
		input    string
		wantType string
		wantOK   bool
	}{
		{
			name:     "Valid IPv4",
			input:    "192.168.1.1",
			wantType: "net.IP",
			wantOK:   true,
		},
		{
			name:     "Valid IPv6",
			input:    "2001:db8::1",
			wantType: "net.IP",
			wantOK:   true,
		},
		{
			name:     "Valid CIDR",
			input:    "192.168.1.0/24",
			wantType: "*net.IPNet",
			wantOK:   true,
		},
		{
			name:     "Valid MAC address",
			input:    "01:23:45:67:89:ab",
			wantType: "net.HardwareAddr",
			wantOK:   true,
		},
		{
			name:     "Regular string",
			input:    "just a string",
			wantType: "",
			wantOK:   false,
		},
		{
			name:     "Number-like string",
			input:    "12345",
			wantType: "",
			wantOK:   false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result, ok := tryParseAs(tt.input)

			assert.Equal(t, tt.wantOK, ok)
			if tt.wantOK {
				assert.Equal(t, tt.wantType, reflect.TypeOf(result).String())
			}
		})
	}
}

func mustParseMac(s string) net.HardwareAddr {
	mac, err := net.ParseMAC(s)
	if err != nil {
		panic(err)
	}
	return mac
}
