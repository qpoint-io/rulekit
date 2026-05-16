package rulekit

import (
	"net"
	"regexp"
	"strings"
)

func compareString(left string, op int, right any) compareOutcome {
	switch right := right.(type) {
	case string:
		// string ? string
		return compareStringString(left, op, right)
	case *regexp.Regexp:
		// string ? regexp
		return compareStringRegex(left, op, right)
	case net.IP:
		// string ? ip
		return compareStringString(left, op, right.String())
	case *net.IPNet:
		// string ? ipnet
		return compareStringString(left, op, right.String())
	case HexString:
		// string ? hex
		return compareBytesBytes([]byte(left), op, right.Bytes)
	}
	return incomparable()
}

func compareStringString(left string, op int, right string) compareOutcome {
	switch op {
	case op_EQ:
		return comparePass(left == right)
	case op_NE:
		return comparePass(left != right)
	case op_CONTAINS:
		return comparePass(strings.Contains(left, right))
	}
	return unsupportedOperator()
}

func compareStringRegex(left string, op int, right *regexp.Regexp) compareOutcome {
	switch op {
	case op_EQ, op_CONTAINS:
		return comparePass(right.MatchString(left))
	case op_NE:
		return comparePass(!right.MatchString(left))
	}
	return unsupportedOperator()
}

func compareStringSlice(left []string, op int, right any) compareOutcome {
	if op == op_CONTAINS {
		// possible options:
		// []string{...} contains string
		// 		-> check if the slice contains the string, not if any of the slice elements contains the string as a substring
		// []string{...} contains regexp
		// 		-> check if the slice contains any element that matches the regexp
		op = op_EQ
	}

	switch right := right.(type) {
	case string:
		// []string{...} ? string
		return compareSliceDetailed(left, op, func(fv string, op int) compareOutcome {
			return compareString(fv, op, right)
		})
	case *regexp.Regexp:
		// []string{...} ? regexp
		return compareSliceDetailed(left, op, func(fv string, op int) compareOutcome {
			return compareStringRegex(fv, op, right)
		})
	}
	return incomparable()
}
