package rulekit

import (
	"net"
	"regexp"
)

func compareMac(left net.HardwareAddr, op int, right any) compareOutcome {
	switch right := right.(type) {
	case net.HardwareAddr:
		// mac ? mac
		return compareBytesBytes(left, op, right)
	case HexString:
		// mac ? hex
		return compareBytesBytes(left, op, right.Bytes)
	case []byte:
		// mac ? bytes
		return compareBytesBytes(left, op, right)
	case string, *regexp.Regexp, urlQuery:
		// mac ? string: compare the MAC's text form
		return compareString(macText(left), op, right)
	}
	return incomparable()
}
