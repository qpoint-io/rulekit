package rulekit

import (
	"fmt"
	"net"
	"strings"

	"github.com/qpoint-io/rulekit/set"
)

type Valuer interface {
	Value(map[string]any) (any, bool)
	String() string
}

type FieldValue string

func (f FieldValue) Value(m map[string]any) (any, bool) {
	return mapPath(m, string(f))
}

func (f FieldValue) String() string {
	return string(f)
}

type LiteralValue[T any] struct {
	raw   string
	value T
}

func (l *LiteralValue[T]) Value(m map[string]any) (any, bool) {
	return l.value, true
}

func (l *LiteralValue[T]) String() string {
	return l.raw
}

func valuerToMissingFields(rv Valuer) set.Set[string] {
	if v, ok := rv.(FieldValue); ok {
		return set.NewSet(string(v))
	}
	return nil
}

type FunctionValue struct {
	fn   string
	args []Valuer
	raw  string
}

func (f *FunctionValue) Value(m map[string]any) (any, bool) {
	// WIP testing function parsing
	fmt.Printf("%s(...)\n", f.fn)
	for _, arg := range f.args {
		fmt.Printf(": %T %s\n", arg, arg.String())
	}
	return nil, false
}

func (f *FunctionValue) String() string {
	return f.raw
}

func isZero(val any) bool {
	switch v := val.(type) {
	case bool:
		return !v
	case int:
		return v == 0
	case int64:
		return v == 0
	case uint:
		return v == 0
	case uint64:
		return v == 0
	case float32:
		return v == 0
	case float64:
		return v == 0
	case string:
		return v == ""
	case []byte:
		return len(v) == 0
	case net.IP:
		return len(v) == 0
	case net.HardwareAddr:
		return len(v) == 0
	case *net.IPNet:
		return v == nil || v.IP == nil
	case []any:
		return len(v) == 0
	}
	return false
}

// mapPath gets element key from a map, interpreting it as a path if it contains a period.
func mapPath(m map[string]any, key string) (any, bool) {
	// Iterative approach to traverse the path
	currentMap := m
	start := 0

	for {
		part := key[start:]
		// First check for direct key match (most common case)
		if val, ok := currentMap[part]; ok {
			return val, true
		}

		// Find the next period
		idx := strings.IndexByte(part, '.')
		if idx == -1 {
			// No more periods, this is the last part
			part = key[start:]
			val, ok := currentMap[part]
			return val, ok
		}

		// Adjust idx to be relative to the full string
		idx += start
		part = key[start:idx]

		// Get the value for this part
		val, ok := currentMap[part]
		if !ok {
			return nil, false
		}

		// Convert to map for next iteration
		nextMap, ok := val.(map[string]any)
		if !ok {
			return nil, false
		}
		currentMap = nextMap

		// Move to the next part
		start = idx + 1
	}
}
