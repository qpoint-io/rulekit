package rulekit

import (
	"net"
	"regexp"
	"strings"

	"github.com/qpoint-io/rulekit/set"
)

type FieldValue string

func (f FieldValue) Eval(ctx *Ctx) Result {
	val, ok := IndexKV(ctx.KV, string(f))
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

func (f FieldValue) ASTNode() ASTNode {
	return &ASTNodeField{
		Name: string(f),
	}
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

func (l *LiteralValue[T]) ASTNode() ASTNode {
	switch v := any(l.value).(type) {
	case bool:
		return &ASTNodeLiteral{Type: "bool", Value: v}
	case int, int64:
		return &ASTNodeLiteral{Type: "int64", Value: v}
	case uint, uint64:
		return &ASTNodeLiteral{Type: "uint64", Value: v}
	case float32, float64:
		return &ASTNodeLiteral{Type: "float64", Value: v}
	case string:
		return &ASTNodeLiteral{Type: "string", Value: v}
	case []byte:
		return &ASTNodeLiteral{Type: "bytes", Value: v}
	case HexString:
		return &ASTNodeLiteral{Type: "hexstring", Value: v}
	case net.IP:
		return &ASTNodeLiteral{Type: "ip", Value: IPString(v)}
	case net.HardwareAddr:
		return &ASTNodeLiteral{Type: "mac", Value: v}
	case *net.IPNet:
		return &ASTNodeLiteral{Type: "cidr", Value: v.String()}
	case []any:
		return &ASTNodeLiteral{Type: "array", Value: v}
	case *regexp.Regexp:
		return &ASTNodeLiteral{Type: "regex", Value: l.raw}
	default:
		return &ASTNodeLiteral{Type: "unknown", Value: l.raw}
	}
}

type ArrayValue struct {
	raw  string
	vals []Rule
}

func (a *ArrayValue) Eval(ctx *Ctx) Result {
	vals := make([]any, len(a.vals))
	for i, val := range a.vals {
		res := val.Eval(ctx)
		if !res.Ok() {
			return res
		}
		vals[i] = res.Value
	}
	return Result{
		Value:         vals,
		EvaluatedRule: a,
	}
}

func (a *ArrayValue) String() string {
	return a.raw
}

func (a *ArrayValue) ASTNode() ASTNode {
	elements := make([]ASTNode, len(a.vals))
	for i, val := range a.vals {
		elements[i] = val.ASTNode()
	}
	return &ASTNodeArray{
		Elements: elements,
	}
}

func newArrayValue(vals []Rule) *ArrayValue {
	var raw strings.Builder
	raw.WriteString("[")
	for i, val := range vals {
		if i > 0 {
			raw.WriteString(", ")
		}
		raw.WriteString(val.String())
	}
	raw.WriteString("]")

	return &ArrayValue{
		raw:  raw.String(),
		vals: vals,
	}
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

// IndexKV gets element key from a map, interpreting it as a path if it contains a period.
func IndexKV(m KV, key string) (any, bool) {
	if m == nil {
		return nil, false
	}

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

func IPString(ip net.IP) string {
	if len(ip) == 0 {
		return ""
	}
	return ip.String()
}
