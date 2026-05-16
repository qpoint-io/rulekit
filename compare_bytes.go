package rulekit

import "bytes"

func compareBytesBytes(left []byte, op int, right []byte) (ret compareOutcome) {
	if ruleDebug >= 1 {
		defer func() {
			debugResult(ret.pass, "│ cmpBytByt", "", left, op, right)
		}()
	}
	switch op {
	case op_EQ:
		return comparePass(bytes.Equal(left, right))
	case op_NE:
		return comparePass(!bytes.Equal(left, right))
	case op_CONTAINS:
		return comparePass(bytes.Contains(left, right))
	}
	return unsupportedOperator()
}
