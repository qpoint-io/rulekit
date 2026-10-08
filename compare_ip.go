package rulekit

import (
	"net"
	"regexp"
)

func compareIP(left net.IP, op int, right any) compareOutcome {
	switch right := right.(type) {
	case net.IP:
		// ip ? ip
		switch op {
		case op_EQ:
			return comparePass(left.Equal(right))
		case op_NE:
			return comparePass(!left.Equal(right))
		}
		return unsupportedOperator()
	case *net.IPNet:
		// ip ? ipnet
		switch op {
		case op_EQ, op_CONTAINS:
			return comparePass(right.Contains(left))
		case op_NE:
			return comparePass(!right.Contains(left))
		}
		return unsupportedOperator()
	case string, *regexp.Regexp, urlQuery:
		// ip ? string: compare the IP's text form
		return compareString(ipText(left), op, right)
	}
	return incomparable()
}

func compareIPNet(left *net.IPNet, op int, right any) compareOutcome {
	switch right := right.(type) {
	case net.IP:
		// ipnet ? ip
		switch op {
		case op_EQ, op_CONTAINS:
			return comparePass(left.Contains(right))
		case op_NE:
			return comparePass(!left.Contains(right))
		}
		return unsupportedOperator()
	case string, *regexp.Regexp, urlQuery:
		// ipnet ? string: compare the CIDR's text form
		return compareString(cidrText(left), op, right)
	}
	return incomparable()
}
