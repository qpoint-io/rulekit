package rulekit

import (
	"net"
	"regexp"
	"strings"
)

func compareString(left string, op int, right any) (ret bool, err error) {
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
		return compareStringString(left, op, right.String())
	case *net.IPNet:
		// string ? ipnet
		return compareStringString(left, op, right.String())
	case HexString:
		// string ? hex
		return compareBytesBytes([]byte(left), op, right.Bytes)
	}
	return false, &ErrIncomparable{
		Field:      left,
		FieldValue: left,
		Value:      right,
		Operator:   operatorToString(op),
	}
}

func compareStringString(left string, op int, right string) (ret bool, err error) {
	defer func() {
		debugResult(ret, "│  cmpStrStr", "", left, op, right)
	}()
	switch op {
	case token_TEST_EQ:
		return left == right, nil
	case token_TEST_NE:
		return left != right, nil
	case token_TEST_CONTAINS:
		return strings.Contains(left, right), nil
	}
	return false, nil
}

func compareStringRegex(left string, op int, right *regexp.Regexp) (ret bool, err error) {
	defer func() {
		debugResult(ret, "│ cmpStrRegex", "", left, op, right)
	}()
	switch op {
	case token_TEST_EQ, token_TEST_CONTAINS:
		return right.MatchString(left), nil
	case token_TEST_NE:
		return !right.MatchString(left), nil
	}
	return false, nil
}

func compareStringSlice(left []string, op int, right any) (ret bool, err error) {
	defer func() {
		debugResult(ret, "│ cmp[]Str", "", left, op, right)
	}()
	if op == token_TEST_CONTAINS {
		// possible options:
		// []string{...} contains string
		// 		-> check if the slice contains the string, not if any of the slice elements contains the string as a substring
		// []string{...} contains regexp
		// 		-> check if the slice contains any element that matches the regexp
		op = token_TEST_EQ
	}

	switch right := right.(type) {
	case string:
		// []string ? string
		return compareSlice(left, op, func(lv string, op int) (bool, error) {
			return compareStringString(lv, op, right)
		})
	case *regexp.Regexp:
		// []string ? regexp
		return compareSlice(left, op, func(lv string, op int) (bool, error) {
			return compareStringRegex(lv, op, right)
		})
	}
	return false, &ErrIncomparable{
		Field:      left,
		FieldValue: left,
		Value:      right,
		Operator:   operatorToString(op),
	}
}
