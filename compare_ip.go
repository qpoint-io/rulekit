package rulekit

import "net"

func compareIP(left net.IP, op int, right any) (ret bool) {
	defer func() {
		debugResult(ret, "│ cmpIP", "", left, op, right)
	}()
	switch right := right.(type) {
	case net.IP:
		// ip ? ip
		switch op {
		case token_TEST_EQ:
			return left.Equal(right)
		case token_TEST_NE:
			return !left.Equal(right)
		}
	case *net.IPNet:
		// ip ? ipnet
		switch op {
		case token_TEST_EQ:
			return right.Contains(left)
		case token_TEST_NE:
			return !right.Contains(left)
		}
	}
	return false
}
