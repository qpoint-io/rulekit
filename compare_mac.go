package rulekit

import (
	"net"
	"strings"
)

func compareMac(left net.HardwareAddr, op int, right any) compareOutcome {
	switch right := right.(type) {
	case net.HardwareAddr:
		// mac ? mac
		return compareBytesBytes(left, op, right)
	case HexString:
		// mac ? hex
		// in this case, treat the hex string as a literal
		return compareStringString(strings.ToLower(left.String()), op, strings.ToLower(right.String()))
	case string:
		// mac ? string
		return compareStringString(strings.ToLower(left.String()), op, strings.ToLower(right))
	}
	return incomparable()
}
