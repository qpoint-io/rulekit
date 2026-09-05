package rulekit

import (
	"fmt"
	"net"
)

type compareDiagnostic uint8

const (
	compareDiagnosticNone compareDiagnostic = iota
	compareDiagnosticIncomparable
	compareDiagnosticInvalidShape
	compareDiagnosticUnsupportedOperator
)

type compareOutcome struct {
	pass       bool
	diagnostic compareDiagnostic
}

func compare(left any, op int, right any) (ret bool) {
	return compareDetailed(left, op, right).pass
}

func compareDetailed(left any, op int, right any) compareOutcome {
	// any ? []any
	//      -> run the comparison for each element in the right array.
	if rightArr, ok := right.([]any); ok {
		if op == op_CONTAINS {
			// the contains operator does not support arrays on the right side.
			return invalidShape()
		}

		return compareSliceDetailed(rightArr, op, func(rv any, op int) compareOutcome {
			return compareDetailed(left, op, rv)
		})
	}

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
		return compareSliceDetailed(lv, op, func(lv int, op int) compareOutcome {
			return compareNumber(lv, op, right)
		})

	case []int64:
		// []int64 ? any
		return compareSliceDetailed(lv, op, func(lv int64, op int) compareOutcome {
			return compareNumber(lv, op, right)
		})

	case []uint:
		// []uint ? any
		return compareSliceDetailed(lv, op, func(lv uint, op int) compareOutcome {
			return compareNumber(lv, op, right)
		})

	case []uint64:
		// []uint64 ? any
		return compareSliceDetailed(lv, op, func(lv uint64, op int) compareOutcome {
			return compareNumber(lv, op, right)
		})

	case []float32:
		// []float32 ? any
		return compareSliceDetailed(lv, op, func(lv float32, op int) compareOutcome {
			return compareNumber(lv, op, right)
		})

	case []float64:
		// []float64 ? any
		return compareSliceDetailed(lv, op, func(lv float64, op int) compareOutcome {
			return compareNumber(lv, op, right)
		})

	case bool:
		rv, ok := right.(bool)
		if !ok {
			return incomparable()
		}
		return compareBool(lv, op, rv)

	case net.IP:
		// ip ? any
		return compareIP(lv, op, right)

	case []net.IP:
		// []net.IP ? any
		return compareSliceDetailed(lv, op, func(lv net.IP, op int) compareOutcome {
			return compareIP(lv, op, right)
		})

	case *net.IPNet:
		// ipnet ? any
		return compareIPNet(lv, op, right)

	case net.HardwareAddr:
		// mac ? any
		return compareMac(lv, op, right)

	case []any:
		// []any ? any
		return compareSliceDetailed(lv, op, func(lv any, op int) compareOutcome {
			return compareDetailed(lv, op, right)
		})
	}

	return incomparable()
}

// compareAnyElem runs fn against each element of a list-valued operand and
// passes if ANY element passes. Reports false if value is not a supported list type.
func compareAnyElem(value any, fn func(el any) compareOutcome) (compareOutcome, bool) {
	each := func(el any, _ int) compareOutcome { return fn(el) }
	switch v := value.(type) {
	case []any:
		return compareSliceDetailed(v, op_EQ, each), true
	case []string:
		return compareSliceDetailed(v, op_EQ, func(el string, op int) compareOutcome { return fn(el) }), true
	case []int:
		return compareSliceDetailed(v, op_EQ, func(el int, op int) compareOutcome { return fn(el) }), true
	case []int64:
		return compareSliceDetailed(v, op_EQ, func(el int64, op int) compareOutcome { return fn(el) }), true
	case []uint:
		return compareSliceDetailed(v, op_EQ, func(el uint, op int) compareOutcome { return fn(el) }), true
	case []uint64:
		return compareSliceDetailed(v, op_EQ, func(el uint64, op int) compareOutcome { return fn(el) }), true
	case []float32:
		return compareSliceDetailed(v, op_EQ, func(el float32, op int) compareOutcome { return fn(el) }), true
	case []float64:
		return compareSliceDetailed(v, op_EQ, func(el float64, op int) compareOutcome { return fn(el) }), true
	case []net.IP:
		return compareSliceDetailed(v, op_EQ, func(el net.IP, op int) compareOutcome { return fn(el) }), true
	}
	return compareOutcome{}, false
}

func comparePass(pass bool) compareOutcome {
	return compareOutcome{pass: pass}
}

func incomparable() compareOutcome {
	return compareOutcome{diagnostic: compareDiagnosticIncomparable}
}

func unsupportedOperator() compareOutcome {
	return compareOutcome{diagnostic: compareDiagnosticUnsupportedOperator}
}

func invalidShape() compareOutcome {
	return compareOutcome{diagnostic: compareDiagnosticInvalidShape}
}

func newComparisonDiagnostic(code compareDiagnostic, left any, op int, right any) Diagnostic {
	leftType := diagnosticType(left)
	rightType := diagnosticType(right)
	operator := operatorToString(op)

	switch code {
	case compareDiagnosticIncomparable:
		return Diagnostic{
			Code:      DiagnosticComparisonIncomparable,
			Message:   fmt.Sprintf("cannot compare %s %s %s", leftType, operator, rightType),
			LeftType:  leftType,
			Operator:  operator,
			RightType: rightType,
		}
	case compareDiagnosticInvalidShape:
		return Diagnostic{
			Code:      DiagnosticComparisonInvalidShape,
			Message:   fmt.Sprintf("invalid comparison shape for %s %s %s", leftType, operator, rightType),
			LeftType:  leftType,
			Operator:  operator,
			RightType: rightType,
		}
	case compareDiagnosticUnsupportedOperator:
		return Diagnostic{
			Code:      DiagnosticComparisonUnsupportedOperator,
			Message:   fmt.Sprintf("operator %s is not supported for %s and %s", operator, leftType, rightType),
			LeftType:  leftType,
			Operator:  operator,
			RightType: rightType,
		}
	default:
		return Diagnostic{}
	}
}

func diagnosticType(value any) string {
	if value == nil {
		return "<nil>"
	}
	return fmt.Sprintf("%T", value)
}
func compareSlice[T any](slice []T, op int, fn func(el T, op int) bool) bool {
	return compareSliceDetailed(slice, op, func(el T, op int) compareOutcome {
		return comparePass(fn(el, op))
	}).pass
}

func compareSliceDetailed[T any](slice []T, op int, fn func(el T, op int) compareOutcome) compareOutcome {
	if op == op_NE {
		// []T != any
		//      -> check if NONE of the slice elements are equal to the right value.
		//         this is equivalent to !([]T == any)
		eq := compareSliceDetailed(slice, op_EQ, fn)
		eq.pass = !eq.pass
		return eq
	}

	if op == op_CONTAINS {
		// []T contains any
		//      -> check if any of the slice elements are equal to the right value.
		//         e.g. we don't want to do any substring matching here.
		op = op_EQ
	}

	var diagnostic compareDiagnostic
	var comparable bool
	for _, el := range slice {
		outcome := fn(el, op)
		if outcome.diagnostic == compareDiagnosticNone {
			comparable = true
		} else if diagnostic == compareDiagnosticNone {
			diagnostic = outcome.diagnostic
		}
		if outcome.pass {
			return comparePass(true)
		}
	}
	if comparable || len(slice) == 0 {
		return comparePass(false)
	}
	return compareOutcome{diagnostic: diagnostic}
}
