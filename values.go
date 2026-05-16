package rulekit

import (
	"net"
	"strconv"
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

func (f FieldValue) Print(PrintMode) string {
	return f.String()
}

// PathValue evaluates an explicit map/slice path, including bracket key and
// numeric index segments.
type PathValue struct {
	segments []pathSegment
}

type pathSegment struct {
	key     string
	index   int
	isIndex bool
	bracket bool
}

func (p *PathValue) Eval(ctx *Ctx) Result {
	val, ok := indexPath(ctx.KV, p.segments)
	if !ok {
		return Result{
			Error:         &ErrMissingFields{Fields: set.NewSet(p.String())},
			EvaluatedRule: p,
		}
	}
	return Result{
		Value:         val,
		EvaluatedRule: p,
	}
}

func (p *PathValue) String() string {
	var raw strings.Builder
	for i, seg := range p.segments {
		if seg.isIndex {
			raw.WriteString("[")
			raw.WriteString(strconv.Itoa(seg.index))
			raw.WriteString("]")
			continue
		}

		if seg.bracket || !isIdentifierSegment(seg.key) {
			raw.WriteString("[")
			raw.WriteString(strconv.Quote(seg.key))
			raw.WriteString("]")
			continue
		}

		if i > 0 {
			raw.WriteString(".")
		}
		raw.WriteString(seg.key)
	}
	return raw.String()
}

func (p *PathValue) Print(PrintMode) string {
	return p.String()
}

func asPathValue(r Rule) (*PathValue, bool) {
	switch v := r.(type) {
	case FieldValue:
		return &PathValue{segments: fieldPathSegments(string(v))}, true
	case *PathValue:
		return v, true
	default:
		return nil, false
	}
}

func fieldPathSegments(path string) []pathSegment {
	parts := strings.Split(path, ".")
	segments := make([]pathSegment, 0, len(parts))
	for _, part := range parts {
		segments = append(segments, pathSegment{key: part})
	}
	return segments
}

func parsePathKey(raw string) (string, error) {
	str := raw
	if str[0] == '\'' {
		str = str[1 : len(str)-1]
		str = strings.ReplaceAll(str, `"`, `\"`)
		str = strings.ReplaceAll(str, `\'`, `'`)
		str = `"` + str + `"`
	}
	return strconv.Unquote(str)
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

func (l *LiteralValue[T]) Print(PrintMode) string {
	return l.String()
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

func (a *ArrayValue) Print(PrintMode) string {
	return a.String()
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

// IndexKV gets a value from a map by interpreting periods as explicit path traversal.
func IndexKV(m KV, key string) (any, bool) {
	if m == nil {
		return nil, false
	}
	if !strings.Contains(key, ".") {
		val, ok := m[key]
		return val, ok
	}
	return indexPath(m, fieldPathSegments(key))
}

func indexPath(m KV, segments []pathSegment) (any, bool) {
	if m == nil || len(segments) == 0 {
		return nil, false
	}

	var current any = map[string]any(m)
	for _, seg := range segments {
		if seg.isIndex {
			val, ok := indexAny(current, seg.index)
			if !ok {
				return nil, false
			}
			current = val
			continue
		}

		currentMap, ok := current.(map[string]any)
		if !ok {
			return nil, false
		}
		val, ok := currentMap[seg.key]
		if !ok {
			return nil, false
		}
		current = val
	}
	return current, true
}

func indexAny(value any, index int) (any, bool) {
	if index < 0 || value == nil {
		return nil, false
	}
	switch v := value.(type) {
	case []any:
		return indexSlice(v, index)
	case []KV:
		return indexSlice(v, index)
	case []string:
		return indexSlice(v, index)
	case []bool:
		return indexSlice(v, index)
	case []int:
		return indexSlice(v, index)
	case []int8:
		return indexSlice(v, index)
	case []int16:
		return indexSlice(v, index)
	case []int32:
		return indexSlice(v, index)
	case []int64:
		return indexSlice(v, index)
	case []uint:
		return indexSlice(v, index)
	case []uint8:
		return indexSlice(v, index)
	case []uint16:
		return indexSlice(v, index)
	case []uint32:
		return indexSlice(v, index)
	case []uint64:
		return indexSlice(v, index)
	case []float32:
		return indexSlice(v, index)
	case []float64:
		return indexSlice(v, index)
	case []net.IP:
		return indexSlice(v, index)
	case []net.HardwareAddr:
		return indexSlice(v, index)
	case []*net.IPNet:
		return indexSlice(v, index)
	default:
		return nil, false
	}
}

func indexSlice[T any](values []T, index int) (any, bool) {
	if index >= len(values) {
		return nil, false
	}
	return values[index], true
}

func isIdentifierSegment(s string) bool {
	return s != "" && !strings.Contains(s, ".")
}
