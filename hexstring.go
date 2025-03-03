package rule

import (
	"encoding/hex"
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

func ParseHexString(s string) (HexString, error) {
	decoded, err := hex.DecodeString(strings.ReplaceAll(s, ":", ""))
	if err != nil {
		return HexString{}, err
	}
	return HexString{
		raw_value: s,
		Bytes:     decoded,
	}, nil
}
