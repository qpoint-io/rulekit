package rulekit

import (
	"fmt"
	"net"
)

func compare(left any, op int, right any) (ret bool, err error) {
	defer func() {
		debugResult(ret, "╰ cmp", "", left, op, right)
	}()
	// the field type determines the comparison logic
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
		return compareSlice(lv, op, func(lv int, op int) (bool, error) {
			return compareNumber(lv, op, right)
		})

	case []int64:
		// []int64 ? any
		return compareSlice(lv, op, func(lv int64, op int) (bool, error) {
			return compareNumber(lv, op, right)
		})

	case []uint:
		// []uint ? any
		return compareSlice(lv, op, func(lv uint, op int) (bool, error) {
			return compareNumber(lv, op, right)
		})

	case []uint64:
		// []uint64 ? any
		return compareSlice(lv, op, func(lv uint64, op int) (bool, error) {
			return compareNumber(lv, op, right)
		})

	case []float32:
		// []float32 ? any
		return compareSlice(lv, op, func(lv float32, op int) (bool, error) {
			return compareNumber(lv, op, right)
		})

	case []float64:
		// []float64 ? any
		return compareSlice(lv, op, func(lv float64, op int) (bool, error) {
			return compareNumber(lv, op, right)
		})

	case bool:
		rv, ok := right.(bool)
		if !ok {
			return false, &ErrIncomparable{
				Field:      left,
				FieldValue: left,
				Value:      right,
				Operator:   operatorToString(op),
			}
		}
		return compareBool(lv, op, rv)

	case net.IP:
		// ip ? any
		return compareIP(lv, op, right)

	case net.HardwareAddr:
		// mac ? any
		return compareMac(lv, op, right)
	}

	return false, &ErrIncomparable{
		Field:      left,
		FieldValue: left,
		Value:      right,
		Operator:   operatorToString(op),
	}
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

func compareSlice[T any](left []T, op int, fn func(lv T, op int) (bool, error)) (bool, error) {
	if op == token_TEST_CONTAINS {
		// []T contains any
		//      -> check if of the slice elements are equal to the right value
		op = token_TEST_EQ
	}

	switch op {
	case token_TEST_EQ,
		token_TEST_GT, token_TEST_GE,
		token_TEST_LT, token_TEST_LE:
		// []T ==, >, >=, <, <= any
		//      -> check if any of the slice elements are equal to the right value
		var lastErr error
		for _, lv := range left {
			// we need only one item to match
			ret, err := fn(lv, op)
			if err != nil {
				lastErr = err
				continue
			}
			if ret {
				return true, nil
			}
		}

		return false, lastErr

	case token_TEST_NE:
		// []T != any
		//      -> check if NONE of the slice elements are equal to the right value
		var lastErr error
		for _, lv := range left {
			ret, err := fn(lv, token_TEST_EQ)
			if err != nil {
				lastErr = err
				continue
			}
			if ret {
				// one item matches
				return false, nil
			}
		}
		return lastErr == nil, lastErr

	default:
		return false, &errIncomparable{
			Left: left,
			// Right:    right, TODO
			Operator: op,
		}
	}
}
