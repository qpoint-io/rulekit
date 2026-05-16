package rulekit

func compareBool(left bool, op int, right bool) compareOutcome {
	switch op {
	case op_EQ:
		return comparePass(left == right)
	case op_NE:
		return comparePass(left != right)
	}
	return unsupportedOperator()
}
