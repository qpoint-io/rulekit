package rulekit

import (
	"fmt"
	"regexp"

	"github.com/qpoint-io/rulekit/set"
)

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
		Pass: rleft.Pass && rright.Pass,
		EvaluatedRule: &nodeAnd{
			left:  rleft.EvaluatedRule,
			right: rright.EvaluatedRule,
		},
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
		Pass: rleft.Pass || rright.Pass,
		EvaluatedRule: &nodeOr{
			left:  rleft.EvaluatedRule,
			right: rright.EvaluatedRule,
		},
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
	if nn, ok := n.right.(*nodeCompare); ok {
		if nn.op == op_EQ {
			// special formatting for !=
			return nn.lv.String() + " != " + nn.rv.String()
		} else if nn.op == op_CONTAINS {
			// special formatting for field not contains "item"
			return nn.lv.String() + " not contains " + nn.rv.String()
		}
	} else if nn, ok := n.right.(*nodeNotZero); ok {
		// special formatting for !FIELD (no space between ! and field)
		return "!" + nn.rv.String()
	} else if nn, ok := n.right.(*nodeMatch); ok {
		// special formatting for field not =~ /pattern/
		return nn.lv.String() + " not =~ " + nn.rv.String()
	} else if nn, ok := n.right.(*nodeIn); ok {
		// special formatting for field not in [1, "str", 3]
		return nn.lv.String() + " not in " + nn.rv.String()
	}

	return "not (" + n.right.String() + ")"
}

// NOT ZERO
type nodeNotZero struct {
	rv KVValuer
}

func (n *nodeNotZero) Eval(p map[string]any) Result {
	val, ok := n.rv.Value(p)
	if !ok {
		return Result{
			// missing field == zero value
			Pass:          false,
			MissingFields: valuerToMissingFields(n.rv),
			EvaluatedRule: n,
		}
	}

	return Result{
		Pass:          !isZero(val),
		EvaluatedRule: n,
	}
}

func (n *nodeNotZero) String() string {
	return n.rv.String()
}

// TEST_MATCHES
type nodeMatch struct {
	lv KVValuer
	rv KVValuer
}

func (n *nodeMatch) Eval(p map[string]any) Result {
	lv, lvOk := n.lv.Value(p)
	rv, rvOk := n.rv.Value(p)
	if !lvOk || !rvOk {
		return Result{
			Pass:          false,
			MissingFields: set.Union(valuerToMissingFields(n.lv), valuerToMissingFields(n.rv)),
			EvaluatedRule: n,
		}
	}

	return Result{
		Pass:          n.apply(lv, rv),
		EvaluatedRule: n,
	}
}

func (n *nodeMatch) apply(lv any, rv any) bool {
	r, ok := rv.(*regexp.Regexp)
	if !ok || r == nil {
		return false
	}

	switch val := lv.(type) {
	case string:
		return r.MatchString(val)
	case []string:
		for _, s := range val {
			if r.MatchString(s) {
				return true
			}
		}
	}
	return false
}

func (n *nodeMatch) FieldName() string {
	return n.lv.String()
}

func (n *nodeMatch) String() string {
	return n.lv.String() + " =~ " + n.rv.String()
}

// Comparison node
type nodeCompare struct {
	lv KVValuer
	op int // op_EQ, NE, GT, GE, LT, LE, CONTAINS
	rv KVValuer
}

func (n *nodeCompare) Eval(m map[string]any) Result {
	lv, lvOk := n.lv.Value(m)
	rv, rvOk := n.rv.Value(m)
	if !lvOk || !rvOk {
		r := Result{
			MissingFields: set.Union(valuerToMissingFields(n.lv), valuerToMissingFields(n.rv)),
			EvaluatedRule: n,
		}
		// if the operator is !=, we may return true if the field is not present as undefined != any
		if n.op == op_NE {
			r.Pass = true
		}
		return r
	}

	pass := compare(lv, n.op, rv)
	return Result{
		Pass:          pass,
		EvaluatedRule: n,
	}
}

func (n *nodeCompare) String() string {
	return n.lv.String() + " " + operatorToString(n.op) + " " + n.rv.String()
}

// TEST_IN
type nodeIn struct {
	lv KVValuer
	rv KVValuer
}

func (n *nodeIn) Eval(p map[string]any) Result {
	lv, lvOk := n.lv.Value(p)
	rv, rvOk := n.rv.Value(p)
	if !lvOk || !rvOk {
		return Result{
			MissingFields: set.Union(valuerToMissingFields(n.lv), valuerToMissingFields(n.rv)),
			EvaluatedRule: n,
		}
	}

	rvArr, ok := rv.([]any)
	if !ok {
		// the right value must be an array
		return Result{
			Pass:          false,
			EvaluatedRule: n,
		}
	}

	// `FIELD in ARR` == `ARR contains FIELD`
	pass := compare(rvArr, op_CONTAINS, lv)
	return Result{
		Pass:          pass,
		EvaluatedRule: n,
	}
}

func (n *nodeIn) String() string {
	return n.lv.String() + " in " + n.rv.String()
}
