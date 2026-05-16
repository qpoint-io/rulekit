package rulekit

func compareBool(left bool, op int, right bool) (ret compareOutcome) {
	if ruleDebug >= 1 {
		defer func() {
			debugResult(ret.pass, "│ cmpBool", "", left, op, right)
		}()
	}
	switch op {
	case op_EQ:
		return comparePass(left == right)
	case op_NE:
		return comparePass(left != right)
	}
	return unsupportedOperator()
}
