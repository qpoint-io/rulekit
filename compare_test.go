package rule

import (
	"cmp"
	"fmt"
	"math"
	"reflect"
	"slices"
	"testing"

	"github.com/stretchr/testify/require"
)

var cmpMatrix = []any{
	int(1), int(-1), int(math.MaxInt), int(math.MinInt),
	int64(1), int64(-1), int64(math.MaxInt64), int64(math.MinInt64),
	uint(1), uint(0), uint(math.MaxUint), uint(0),
	uint64(1), uint64(0), uint64(math.MaxUint64),
	float32(1.0), float32(-1.0), float32(math.MaxFloat32), float32(math.SmallestNonzeroFloat32),
	float64(1.0), float64(-1.0), float64(math.MaxFloat64), float64(math.SmallestNonzeroFloat64),
	"1",
}

func TestCmpNumber(t *testing.T) {
	for _, x := range cmpMatrix {
		for _, y := range cmpMatrix {
			// swapping x and y should give the opposite result
			require.Equalf(t, cmpNumber(x, y), reversedCmpResult(cmpNumber(y, x)), "asymmetry[%T,%T]", x, y)
			require.Equalf(t, reflectCmpNumber(x, y), cmpNumber(x, y), "correctness[%T,%T]", x, y)

			if cmpNumber(x, y) == cmpResultNotComparable {
				// if x and y are both numeric, cmpNumber may NOT return cmpResultNotComparable
				require.Falsef(t, isNum(x) && isNum(y), "[%T,%T] are both numeric but cmpNumber returned not comparable", x, y)
			}
		}
	}
}

func reflectCmpNumber(x, y any) int {
	xVal := reflect.ValueOf(x)
	yVal := reflect.ValueOf(y)

	// Convert to float64 for comparison
	var xFloat, yFloat float64

	switch xVal.Kind() {
	case reflect.Int, reflect.Int64:
		xFloat = float64(xVal.Int())
	case reflect.Uint, reflect.Uint64:
		xFloat = float64(xVal.Uint())
	case reflect.Float32, reflect.Float64:
		xFloat = xVal.Float()
	default:
		return cmpResultNotComparable
	}

	switch yVal.Kind() {
	case reflect.Int, reflect.Int64:
		yFloat = float64(yVal.Int())
	case reflect.Uint, reflect.Uint64:
		yFloat = float64(yVal.Uint())
	case reflect.Float32, reflect.Float64:
		yFloat = yVal.Float()
	default:
		return cmpResultNotComparable
	}

	return cmp.Compare(xFloat, yFloat)
}

func reversedCmpResult(a int) int {
	switch a {
	case cmpResultEqual:
		return cmpResultEqual
	case cmpResultLess:
		return cmpResultGreater
	case cmpResultGreater:
		return cmpResultLess
	case cmpResultNotComparable:
		return cmpResultNotComparable
	default:
		panic(fmt.Sprintf("unknown cmpResult: %d", a))
	}
}

func isNum(x any) bool {
	return slices.Contains(
		[]reflect.Kind{reflect.Int, reflect.Int8, reflect.Int16, reflect.Int32, reflect.Int64, reflect.Uint, reflect.Uint8, reflect.Uint16, reflect.Uint32, reflect.Uint64, reflect.Float32, reflect.Float64},
		reflect.TypeOf(x).Kind(),
	)
}

func BenchmarkCmpNumber(b *testing.B) {
	compared := map[string]struct{}{}

	for _, x := range cmpMatrix {
		for _, y := range cmpMatrix {
			key := fmt.Sprintf("%T-%T", x, y)
			if _, ok := compared[key]; ok {
				continue
			}

			compared[key] = struct{}{}
			b.Run(key, func(b *testing.B) {
				for range b.N {
					cmpNumber(x, y)
				}
			})
		}
	}
}
