package rulekit

import "cmp"

func compareNumber(left any, op int, right any) (ret bool) {
	defer func() {
		debugResult(ret, "│ cmpNum", "", left, op, right)
	}()

	return compareWithOp(cmpNumber(left, right), op)
}

func cmpNumber(left any, right any) int {
	switch left := left.(type) {
	case int:
		return cmpNumber(int64(left), right)
	case int64:
		switch right := right.(type) {
		case int:
			return cmp.Compare(left, int64(right))
		case int64:
			return cmp.Compare(left, right)
		case uint:
			return compareSignedUnsigned(left, uint64(right))
		case uint64:
			return compareSignedUnsigned(left, right)
		case float32:
			return cmp.Compare(float64(left), float64(right))
		case float64:
			return cmp.Compare(float64(left), right)
		}

	case uint:
		return cmpNumber(uint64(left), right)
	case uint64:
		switch right := right.(type) {
		case int:
			return compareUnsignedSigned(left, int64(right))
		case int64:
			return compareUnsignedSigned(left, right)
		case uint:
			return cmp.Compare(left, uint64(right))
		case uint64:
			return cmp.Compare(left, right)
		case float32:
			return cmp.Compare(float64(left), float64(right))
		case float64:
			return cmp.Compare(float64(left), right)
		}

	case float32:
		return cmpNumber(float64(left), right)
	case float64:
		switch right := right.(type) {
		case float32:
			return cmp.Compare(left, float64(right))
		case float64:
			return cmp.Compare(left, right)
		case int:
			return cmp.Compare(left, float64(right))
		case int64:
			return cmp.Compare(left, float64(right))
		case uint:
			return cmp.Compare(left, float64(right))
		case uint64:
			return cmp.Compare(left, float64(right))
		}
	}
	return cmpResultNotComparable
}

// Helper function for comparing signed vs unsigned numbers
func compareSignedUnsigned(left int64, right uint64) int {
	if left < 0 {
		// negative ? positive
		return cmpResultLess
	}
	return cmp.Compare(uint64(left), right)
}

// Helper function for comparing unsigned vs signed numbers
func compareUnsignedSigned(left uint64, right int64) int {
	if right < 0 {
		// positive ? negative
		return cmpResultGreater
	}
	return cmp.Compare(left, uint64(right))
}

const (
	cmpResultNotComparable = -2
	cmpResultLess          = -1
	cmpResultEqual         = 0
	cmpResultGreater       = 1
)

// Helper function to convert comparison result to boolean based on operator
func compareWithOp(cmpResult int, op int) bool {
	if cmpResult == cmpResultNotComparable {
		return false
	}

	switch op {
	case token_TEST_EQ:
		return cmpResult == cmpResultEqual
	case token_TEST_NE:
		return cmpResult != cmpResultEqual
	case token_TEST_GT:
		return cmpResult == cmpResultGreater
	case token_TEST_GE:
		return cmpResult == cmpResultGreater || cmpResult == cmpResultEqual
	case token_TEST_LT:
		return cmpResult == cmpResultLess
	case token_TEST_LE:
		return cmpResult == cmpResultLess || cmpResult == cmpResultEqual
	}
	return false
}
