package rule

import "bytes"

func compareBytesBytes(left []byte, op int, right []byte) (ret bool) {
	defer func() {
		debugResult(ret, "│ cmpBytByt", "", left, op, right)
	}()
	switch op {
	case token_TEST_EQ:
		return bytes.Equal(left, right)
	case token_TEST_NE:
		return !bytes.Equal(left, right)
	case token_TEST_CONTAINS:
		return bytes.Contains(left, right)
	}
	return false
}
