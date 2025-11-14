package rulekit

import (
	"net"
	"regexp"
	"strings"
)

func compareString(left string, op int, right any) (ret bool) {
	defer func() {
		debugResult(ret, "│ cmpStr", "", left, op, right)
	}()
	switch right := right.(type) {
	case string:
		// string ? string
		return compareStringString(left, op, right)
	case *regexp.Regexp:
		// string ? regexp
		return compareStringRegex(left, op, right)
	case net.IP:
		// string ? ip
		return compareStringString(left, op, IPString(right))
	case *net.IPNet:
		// string ? ipnet
		return compareStringString(left, op, right.String())
	case HexString:
		// string ? hex
		return compareBytesBytes([]byte(left), op, right.Bytes)
	}
	return false
}

func compareStringString(left string, op int, right string) (ret bool) {
	defer func() {
		debugResult(ret, "│  cmpStrStr", "", left, op, right)
	}()
	switch op {
	case op_EQ:
		return left == right
	case op_NE:
		return left != right
	case op_CONTAINS:
		return strings.Contains(left, right)
	}
	return false
}

func compareStringRegex(left string, op int, right *regexp.Regexp) (ret bool) {
	defer func() {
		debugResult(ret, "│ cmpStrRegex", "", left, op, right)
	}()
	switch op {
	case op_EQ, op_CONTAINS:
		return right.MatchString(left)
	case op_NE:
		return !right.MatchString(left)
	}
	return false
}

func compareStringSlice(left []string, op int, right any) (ret bool) {
	defer func() {
		debugResult(ret, "│ cmp[]Str", "", left, op, right)
	}()
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
		for _, fv := range left {
			if compareString(fv, op, right) {
				return true
			}
		}
		return false
	case *regexp.Regexp:
		// []string{...} ? regexp
		for _, fv := range left {
			if compareStringRegex(fv, op, right) {
				return true
			}
		}
		return false
	}
	return false
}
