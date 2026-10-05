package rulekit

import (
	"bytes"
	"net"
)

// compareBytes compares a byte sequence (bytes input or a hex literal) by
// value against bytes, hex, strings, and MAC addresses.
func compareBytes(left []byte, op int, right any) compareOutcome {
	switch right := right.(type) {
	case []byte:
		return compareBytesBytes(left, op, right)
	case HexString:
		return compareBytesBytes(left, op, right.Bytes)
	case net.HardwareAddr:
		return compareBytesBytes(left, op, right)
	case string:
		switch op {
		case op_EQ:
			return comparePass(string(left) == right)
		case op_NE:
			return comparePass(string(left) != right)
		}
		return compareBytesBytes(left, op, []byte(right))
	}
	return incomparable()
}

func compareBytesBytes(left []byte, op int, right []byte) compareOutcome {
	switch op {
	case op_EQ:
		return comparePass(bytes.Equal(left, right))
	case op_NE:
		return comparePass(!bytes.Equal(left, right))
	case op_CONTAINS:
		return comparePass(bytes.Contains(left, right))
	}
	return unsupportedOperator()
}
