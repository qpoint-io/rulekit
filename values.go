package rulekit

import (
	"net"
	"strings"

	"github.com/qpoint-io/rulekit/set"
)

type FieldValue string

func (f FieldValue) Eval(ctx *Ctx) Result {
	val, ok := mapPath(ctx.KV, string(f))
	if !ok {
		return Result{
			Error:         &ErrMissingFields{Fields: set.NewSet(string(f))},
			EvaluatedRule: f,
		}
	}
	return Result{
		Value:         val,
		EvaluatedRule: f,
	}
}

func (f FieldValue) String() string {
	return string(f)
}

type LiteralValue[T any] struct {
	raw   string
	value T
}

func (l *LiteralValue[T]) Eval(ctx *Ctx) Result {
	return Result{
		Value:         l.value,
		EvaluatedRule: l,
	}
}

func (l *LiteralValue[T]) String() string {
	return l.raw
}

func isZero(val any) bool {
	if val == nil {
		return true
	}

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
