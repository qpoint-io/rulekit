package rulekit

func compareBool(left bool, op int, right bool) (ret bool) {
	if ruleDebug >= 1 {
		defer func() {
			debugResult(ret, "│ cmpBool", "", left, op, right)
		}()
	}
	switch op {
	case op_EQ:
		return left == right
	case op_NE:
		return left != right
	}
	return false
}
