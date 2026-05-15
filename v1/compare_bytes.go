package rulekit

import "bytes"

func compareBytesBytes(left []byte, op int, right []byte) (ret bool) {
	defer func() {
		debugResult(ret, "│ cmpBytByt", "", left, op, right)
	}()
	switch op {
	case op_EQ:
		return bytes.Equal(left, right)
	case op_NE:
		return !bytes.Equal(left, right)
	case op_CONTAINS:
		return bytes.Contains(left, right)
	}
	return false
}
