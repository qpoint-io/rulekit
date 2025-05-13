package rulekit

import (
	"fmt"
	"regexp"
)

// AND
type nodeAnd struct {
	left  Rule
	right Rule
}

func (n *nodeAnd) Eval(p map[string]any) Result {
	// if either node fails, return only that node
	rleft := n.left.Eval(p)
	if rleft.Fail() {
		return rleft
	}

	rright := n.right.Eval(p)
	if rright.Fail() {
		return rright
	}

	// if only one node is not ok, return it
	if rleft.Ok() && !rright.Ok() {
		return rright
	} else if !rleft.Ok() && rright.Ok() {
		return rleft
	}

	// at this point either both nodes are ok or both are not ok.
	var value any
	if rleft.Ok() && rright.Ok() {
		// set the result only if both nodes are ok
		value = rleft.Pass() && rright.Pass()
	}

	return Result{
		Value: value,
		EvaluatedRule: &nodeAnd{
			left:  rleft.EvaluatedRule,
			right: rright.EvaluatedRule,
		},
		Error: coalesceErrs(rleft.Error, rright.Error),
	}
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
	// if either node passes, return only that node
	rleft := n.left.Eval(p)
	if rleft.Pass() {
		return rleft
	}

	rright := n.right.Eval(p)
	if rright.Pass() {
		return rright
	}

	// if only one node is not ok, return it
	if rleft.Ok() && !rright.Ok() {
		return rright
	} else if !rleft.Ok() && rright.Ok() {
		return rleft
	}

	// at this point either both nodes are ok or both are not ok.
	var value any
	if rleft.Ok() && rright.Ok() {
		// set the result only if both nodes are ok
		value = rleft.Pass() || rright.Pass()
	}

	return Result{
		Value: value,
		EvaluatedRule: &nodeOr{
			left:  rleft.EvaluatedRule,
			right: rright.EvaluatedRule,
		},
		Error: coalesceErrs(rleft.Error, rright.Error),
	}
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

	res := Result{
		EvaluatedRule: n,
	}
	if r.Ok() {
		res.Value = !r.Pass()
	} else {
		res.Error = r.Error
	}

	return res
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
	rv Valuer
}

func (n *nodeNotZero) Eval(p map[string]any) Result {
	val, ok := n.rv.Value(p)
	if !ok {
		return Result{
			Error:         valuersToMissingFields(n.rv),
			EvaluatedRule: n,
		}
	}

	return Result{
		Value:         !isZero(val),
		EvaluatedRule: n,
	}
}

func (n *nodeNotZero) String() string {
	return n.rv.String()
}

// TEST_MATCHES
type nodeMatch struct {
	lv Valuer
	rv Valuer
}

func (n *nodeMatch) Eval(p map[string]any) Result {
	lv, lvOk := n.lv.Value(p)
	rv, rvOk := n.rv.Value(p)
	if !lvOk || !rvOk {
		return Result{
			Error:         valuersToMissingFields(n.lv, n.rv),
			EvaluatedRule: n,
		}
	}

	return Result{
		Value:         n.apply(lv, rv),
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
	lv Valuer
	op int // op_EQ, NE, GT, GE, LT, LE, CONTAINS
	rv Valuer
}

func (n *nodeCompare) Eval(m map[string]any) Result {
	lv, lvOk := n.lv.Value(m)
	rv, rvOk := n.rv.Value(m)
	if !lvOk || !rvOk {
		r := Result{
			Error:         valuersToMissingFields(n.lv, n.rv),
			EvaluatedRule: n,
		}
		return r
	}

	pass := compare(lv, n.op, rv)
	return Result{
		Value:         pass,
		EvaluatedRule: n,
	}
}

func (n *nodeCompare) String() string {
	return n.lv.String() + " " + operatorToString(n.op) + " " + n.rv.String()
}

// TEST_IN
type nodeIn struct {
	lv Valuer
	rv Valuer
}

func (n *nodeIn) Eval(p map[string]any) Result {
	lv, lvOk := n.lv.Value(p)
	rv, rvOk := n.rv.Value(p)
	if !lvOk || !rvOk {
		return Result{
			Error:         valuersToMissingFields(n.lv, n.rv),
			EvaluatedRule: n,
		}
	}

	rvArr, ok := rv.([]any)
	if !ok {
		// the right value must be an array
		return Result{
			EvaluatedRule: n,
		}
	}

	// `FIELD in ARR` == `ARR contains FIELD`
	pass := compare(rvArr, op_CONTAINS, lv)
	return Result{
		Value:         pass,
		EvaluatedRule: n,
	}
}

func (n *nodeIn) String() string {
	return n.lv.String() + " in " + n.rv.String()
}
