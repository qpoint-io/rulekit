package rulekit

import (
	"fmt"
	"net"
)

func compare(left any, op int, right any) (ret bool) {
	// any ? []any
	//      -> run the comparison for each element in the right array.
	if rightArr, ok := right.([]any); ok {
		defer func() {
			debugResult(ret, "╰ cmp[]", "", left, op, right)
		}()

		if op == token_TEST_CONTAINS {
			// the contains operator does not support arrays on the right side.
			// TODO: return error
			return false
		}

		return compareSlice(rightArr, op, func(rv any, op int) bool {
			return compare(left, op, rv)
		})
	}

	defer func() {
		debugResult(ret, "╰ cmp", "", left, op, right)
	}()

	// the left value type determines the comparison logic
	switch lv := left.(type) {
	case string:
		// string ? any
		return compareString(lv, op, right)

	case []string:
		// []string ? any
		return compareStringSlice(lv, op, right)

	case int, int64, uint, uint64, float32, float64:
		// int ? any
		return compareNumber(lv, op, right)

	case []int:
		// []int ? any
		return compareSlice(lv, op, func(lv int, op int) bool {
			return compareNumber(lv, op, right)
		})

	case []int64:
		// []int64 ? any
		return compareSlice(lv, op, func(lv int64, op int) bool {
			return compareNumber(lv, op, right)
		})

	case []uint:
		// []uint ? any
		return compareSlice(lv, op, func(lv uint, op int) bool {
			return compareNumber(lv, op, right)
		})

	case []uint64:
		// []uint64 ? any
		return compareSlice(lv, op, func(lv uint64, op int) bool {
			return compareNumber(lv, op, right)
		})

	case []float32:
		// []float32 ? any
		return compareSlice(lv, op, func(lv float32, op int) bool {
			return compareNumber(lv, op, right)
		})

	case []float64:
		// []float64 ? any
		return compareSlice(lv, op, func(lv float64, op int) bool {
			return compareNumber(lv, op, right)
		})

	case bool:
		rv, ok := right.(bool)
		if !ok {
			return false
		}
		return compareBool(lv, op, rv)

	case net.IP:
		// ip ? any
		return compareIP(lv, op, right)

	case *net.IPNet:
		// ipnet ? any
		return compareIPNet(lv, op, right)

	case net.HardwareAddr:
		// mac ? any
		return compareMac(lv, op, right)

	case []any:
		// []any ? any
		return compareSlice(lv, op, func(lv any, op int) bool {
			return compare(lv, op, right)
		})
	}

	return false
}

func debugResult(result bool, prefix string, lname string, lv any, op int, rv any) bool {
	if ruleDebug >= 1 {
		lvTxt := fmt.Sprintf("[%T] %v", lv, lv)
		if lname != "" {
			lvTxt = fmt.Sprintf("%s(%s)", lname, lvTxt)
		}
		fmt.Fprintf(ruleDebugWriter, "%-20s%5t:\t%s %s [%T] %v\n", prefix, result, lvTxt, operatorToString(op), rv, rv)
	}
	return result
}

func compareSlice[T any](slice []T, op int, fn func(el T, op int) bool) bool {
	if op == token_TEST_NE {
		// []T != any
		//      -> check if NONE of the slice elements are equal to the right value.
		//         this is equivalent to !([]T == any)
		return !compareSlice(slice, token_TEST_EQ, fn)
	}

	if op == token_TEST_CONTAINS {
		// []T contains any
		//      -> check if any of the slice elements are equal to the right value.
		//         e.g. we don't want to do any substring matching here.
		op = token_TEST_EQ
	}

	for _, el := range slice {
		if fn(el, op) {
			return true
		}
	}
	return false
}
