package rulekit

import "bytes"

func compareBytesBytes(left []byte, op int, right []byte) (ret bool, err error) {
	defer func() {
		debugResult(ret, "│ cmpBytByt", "", left, op, right)
	}()
	switch op {
	case token_TEST_EQ:
		return bytes.Equal(left, right), nil
	case token_TEST_NE:
		return !bytes.Equal(left, right), nil
	case token_TEST_CONTAINS:
		return bytes.Contains(left, right), nil
	}
	return false, nil
}
