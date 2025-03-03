package rule

import (
	"fmt"
	"net"
	"regexp"

	"github.com/qpoint-io/rulekit/set"
)

type predicate struct {
	field     string
	raw_value string
}

// AND
type nodeAnd struct {
	left  Rule
	right Rule
}

func (n *nodeAnd) Eval(p map[string]any) Result {
	rleft := n.left.Eval(p)
	rright := n.right.Eval(p)

	// if either node fails strictly, return only that node
	if rleft.FailStrict() {
		return rleft
	} else if rright.FailStrict() {
		return rright
	}

	// either one could pass/fail with/without missing fields
	r := Result{
		Pass:          rleft.Pass && rright.Pass,
		EvaluatedRule: n,
		MissingFields: set.Union(rleft.MissingFields, rright.MissingFields),
	}
	return r
}

func (n *nodeAnd) String() string {
	return fmt.Sprintf("(%s and %s)", n.left.String(), n.right.String())
}

// OR
type nodeOr struct {
	left  Rule
	right Rule
}

func (n *nodeOr) Eval(p map[string]any) Result {
	rleft := n.left.Eval(p)
	rright := n.right.Eval(p)

	// if either node passes strictly, return only that node
	if rleft.PassStrict() {
		return rleft
	} else if rright.PassStrict() {
		return rright
	}

	// either one could pass/fail with/without missing fields
	r := Result{
		Pass:          rleft.Pass || rright.Pass,
		EvaluatedRule: n,
		MissingFields: set.Union(rleft.MissingFields, rright.MissingFields),
	}
	return r
}

func (n *nodeOr) String() string {
	return fmt.Sprintf("(%s or %s)", n.left.String(), n.right.String())
}

// NOT
type nodeNot struct {
	right Rule
}

func (n *nodeNot) Eval(p map[string]any) Result {
	if n.right == nil {
		return Result{EvaluatedRule: n}
	}

	r := n.right.Eval(p)
	return Result{
		Pass:          !r.Pass,
		MissingFields: r.MissingFields,
		EvaluatedRule: n,
	}
}

func (n *nodeNot) String() string {
	if nn, ok := n.right.(*nodeCompare); ok && nn.op == token_TEST_EQ {
		// special formatting for !=
		return nn.field + " != " + nn.raw_value
	} else if nn, ok := n.right.(*nodeNotZero); ok {
		// special formatting for !FIELD (no space between ! and field)
		return "!" + nn.field
	}
	return "! " + n.right.String()
}

// NOT ZERO
type nodeNotZero struct {
	field string
}

func (n *nodeNotZero) Eval(p map[string]any) Result {
	val, ok := p[n.field]
	if !ok {
		return Result{
			// missing field == zero value
			Pass:          false,
			MissingFields: set.NewSet(n.field),
			EvaluatedRule: n,
		}
	}

	return Result{
		Pass:          !isZero(val),
		EvaluatedRule: n,
	}
}

func (n *nodeNotZero) String() string {
	return n.field
}

// TEST_MATCHES
type nodeMatch struct {
	predicate
	reg_expr *regexp.Regexp
}

func (n *nodeMatch) Eval(p map[string]any) Result {
	val, ok := p[n.field]
	if !ok {
		return Result{
			Pass:          false,
			MissingFields: set.NewSet(n.field),
			EvaluatedRule: n,
		}
	}

	return Result{
		Pass:          n.apply(val),
		EvaluatedRule: n,
	}
}

func (n *nodeMatch) apply(fv any) bool {
	if n.reg_expr == nil {
		return false
	}
	switch val := fv.(type) {
	case string:
		return n.reg_expr.MatchString(val)
	case []string:
		for _, s := range val {
			if n.reg_expr.MatchString(s) {
				return true
			}
		}
	}
	return false
}

func (n *nodeMatch) FieldName() string {
	return n.field
}

func (n *nodeMatch) String() string {
	return n.field + " =~ " + n.raw_value
}

// Comparison node
type nodeCompare struct {
	predicate
	op    int // token_TEST_EQ, NE, GT, GE, LT, LE, CONTAINS
	value any
}

func (n *nodeCompare) Eval(m map[string]any) Result {
	value, ok := m[n.field]
	if !ok {
		r := Result{
			MissingFields: set.NewSet(n.field),
			EvaluatedRule: n,
		}
		// if the operator is !=, we may return true if the field is not present as undefined != any
		if n.op == token_TEST_NE {
			r.Pass = true
		}
		return r
	}

	pass := compare(value, n.op, n.value)
	return Result{
		Pass:          pass,
		EvaluatedRule: n,
	}
}

func (n *nodeCompare) String() string {
	return fmt.Sprintf("%s %s %s", n.field, operatorToString(n.op), n.raw_value)
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
	}
	return false
}
