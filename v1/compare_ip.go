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
		case op_EQ:
			return left.Equal(right)
		case op_NE:
			return !left.Equal(right)
		}
	case *net.IPNet:
		// ip ? ipnet
		switch op {
		case op_EQ, op_CONTAINS:
			return right.Contains(left)
		case op_NE:
			return !right.Contains(left)
		}
	}
	return false
}

func compareIPNet(left *net.IPNet, op int, right any) (ret bool) {
	defer func() {
		debugResult(ret, "│ cmpIPNet", "", left, op, right)
	}()
	switch right := right.(type) {
	case net.IP:
		// ipnet ? ip
		switch op {
		case op_EQ, op_CONTAINS:
			return left.Contains(right)
		case op_NE:
			return !left.Contains(right)
		}
	}
	return false
}
