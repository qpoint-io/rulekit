package rulekit

func compareBool(left bool, op int, right bool) (ret bool, err error) {
	defer func() {
		debugResult(ret, "│ cmpBool", "", left, op, right)
	}()
	switch op {
	case token_TEST_EQ:
		return left == right, nil
	case token_TEST_NE:
		return left != right, nil
	}
	return false, nil
}
