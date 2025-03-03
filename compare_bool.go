package rule

func compareBool(left bool, op int, right bool) (ret bool) {
	defer func() {
		debugResult(ret, "│ cmpBool", "", left, op, right)
	}()
	switch op {
	case token_TEST_EQ:
		return left == right
	case token_TEST_NE:
		return left != right
	}
	return false
}
