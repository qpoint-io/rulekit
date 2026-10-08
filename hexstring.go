package rulekit

import (
	"encoding/hex"
	"fmt"
	"strings"
)

// HexString represents a hex-encoded string retaining the original input string
type HexString struct {
	raw_value string
	Bytes     []byte
}

func (h HexString) String() string {
	return h.raw_value
}

// ParseHexString parses colon-separated hex pairs (50:4f:53:54) or an
// x"..." literal whose digits may optionally be separated by colons.
func ParseHexString(s string) (HexString, error) {
	digits := s
	if len(s) >= 3 && (s[0] == 'x' || s[0] == 'X') && (s[1] == '"' || s[1] == '\'') && s[len(s)-1] == s[1] {
		digits = s[2 : len(s)-1]
		if digits == "" {
			return HexString{}, fmt.Errorf("empty hex literal")
		}
	}
	decoded, err := hex.DecodeString(strings.ReplaceAll(digits, ":", ""))
	if err != nil {
		return HexString{}, err
	}
	return HexString{
		raw_value: s,
		Bytes:     decoded,
	}, nil
}
