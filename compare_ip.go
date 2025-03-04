package rulekit

import (
	"net"
	"strings"
)

func compareIP(left net.IP, op int, right any) (ret bool, err error) {
	defer func() {
		debugResult(ret, "│ cmpIP", "", left, op, right)
	}()
	switch right := right.(type) {
	case string:
		// ip ? string
		return compareStringString(strings.ToLower(left.String()), op, strings.ToLower(right))
	case net.IP:
		// ip ? ip
		switch op {
		case token_TEST_EQ:
			return left.Equal(right), nil
		case token_TEST_NE:
			return !left.Equal(right), nil
		}
	case *net.IPNet:
		// ip ? ipnet
		switch op {
		case token_TEST_EQ, token_TEST_CONTAINS:
			return right.Contains(left), nil
		case token_TEST_NE:
			return !right.Contains(left), nil
		}
	}
	return false, &ErrIncomparable{
		Field:      left,
		FieldValue: left,
		Value:      right,
		Operator:   operatorToString(op),
	}
}
