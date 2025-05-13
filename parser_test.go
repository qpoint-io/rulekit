package rulekit

import (
	"net"
	"reflect"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func Test_parseString(t *testing.T) {
	tests := []struct {
		name     string
		input    string
		wantType string
		wantOK   bool
	}{
		{
			name:     "Regular string",
			input:    `"just a string"`,
			wantType: "string",
			wantOK:   true,
		},
		{
			name:     "Valid IPv4",
			input:    `"192.168.1.1"`,
			wantType: "net.IP",
			wantOK:   true,
		},
		{
			name:     "Valid IPv6",
			input:    `"2001:db8::1"`,
			wantType: "net.IP",
			wantOK:   true,
		},
		{
			name:     "Valid CIDR",
			input:    `"192.168.1.0/24"`,
			wantType: "*net.IPNet",
			wantOK:   true,
		},
		{
			name:     "Valid MAC address",
			input:    `"01:23:45:67:89:ab"`,
			wantType: "net.HardwareAddr",
			wantOK:   true,
		},
		{
			name:     "Number",
			input:    `12345`,
			wantType: "",
			wantOK:   false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result, err := parseString(tt.input)
			if tt.wantOK {
				require.NoError(t, err)
				assert.Equal(t, tt.wantType, reflect.TypeOf(result).String())
			} else {
				require.Error(t, err)
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
