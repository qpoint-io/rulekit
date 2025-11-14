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

func (n *nodeAnd) Eval(ctx *Ctx) Result {
	// if either node fails, return only that node
	rleft := n.left.Eval(ctx)
	if rleft.Fail() {
		return rleft
	}

	rright := n.right.Eval(ctx)
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

func (n *nodeAnd) ASTNode() ASTNode {
	return &ASTNodeOperator{
		Operator: "and",
		Left:     n.left.ASTNode(),
		Right:    n.right.ASTNode(),
	}
}

// OR
type nodeOr struct {
	left  Rule
	right Rule
}

func (n *nodeOr) Eval(ctx *Ctx) Result {
	// if either node passes, return only that node
	rleft := n.left.Eval(ctx)
	if rleft.Pass() {
		return rleft
	}

	rright := n.right.Eval(ctx)
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

func (n *nodeOr) ASTNode() ASTNode {
	return &ASTNodeOperator{
		Operator: "or",
		Left:     n.left.ASTNode(),
		Right:    n.right.ASTNode(),
	}
}

// NOT
type nodeNot struct {
	right Rule
}

func (n *nodeNot) Eval(ctx *Ctx) Result {
	if n.right == nil {
		return Result{EvaluatedRule: n}
	}

	r := n.right.Eval(ctx)
	if !r.Ok() {
		return Result{
			Error:         r.Error,
			EvaluatedRule: n,
		}
	}

	return Result{
		Value:         !isZero(r.Value),
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
	} else if nn, ok := n.right.(FieldValue); ok {
		// special formatting for !FIELD (no space between ! and field)
		return "!" + nn.String()
	} else if nn, ok := n.right.(*nodeMatch); ok {
		// special formatting for field not =~ /pattern/
		return nn.lv.String() + " not =~ " + nn.rv.String()
	} else if nn, ok := n.right.(*nodeIn); ok {
		// special formatting for field not in [1, "str", 3]
		return nn.lv.String() + " not in " + nn.rv.String()
	}

	return "not (" + n.right.String() + ")"
}

func (n *nodeNot) ASTNode() ASTNode {
	return &ASTNodeOperator{
		Operator: "not",
		Right:    n.right.ASTNode(),
	}
}

// TEST_MATCHES
type nodeMatch struct {
	lv Rule
	rv Rule
}

func (n *nodeMatch) Eval(ctx *Ctx) Result {
	lv := n.lv.Eval(ctx)
	if !lv.Ok() {
		return Result{
			Error:         lv.Error,
			EvaluatedRule: n,
		}
	}
	rv := n.rv.Eval(ctx)
	if !rv.Ok() {
		return Result{
			Error:         rv.Error,
			EvaluatedRule: n,
		}
	}

	return Result{
		Value:         n.apply(lv.Value, rv.Value),
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

func (n *nodeMatch) ASTNode() ASTNode {
	return &ASTNodeOperator{
		Operator: "matches",
		Left:     n.lv.ASTNode(),
		Right:    n.rv.ASTNode(),
	}
}

// Comparison node
type nodeCompare struct {
	lv Rule
	op int // op_EQ, NE, GT, GE, LT, LE, CONTAINS
	rv Rule
}

func (n *nodeCompare) Eval(ctx *Ctx) Result {
	lv := n.lv.Eval(ctx)
	if !lv.Ok() {
		return Result{
			Error:         lv.Error,
			EvaluatedRule: n,
		}
	}
	rv := n.rv.Eval(ctx)
	if !rv.Ok() {
		return Result{
			Error:         rv.Error,
			EvaluatedRule: n,
		}
	}

	pass := compare(lv.Value, n.op, rv.Value)
	return Result{
		Value:         pass,
		EvaluatedRule: n,
	}
}

func (n *nodeCompare) String() string {
	return n.lv.String() + " " + operatorToString(n.op) + " " + n.rv.String()
}

func (n *nodeCompare) ASTNode() ASTNode {
	return &ASTNodeOperator{
		Operator: operatorToText(n.op),
		Left:     n.lv.ASTNode(),
		Right:    n.rv.ASTNode(),
	}
}

// TEST_IN
type nodeIn struct {
	lv Rule
	rv Rule
}

func (n *nodeIn) Eval(ctx *Ctx) Result {
	lv := n.lv.Eval(ctx)
	if !lv.Ok() {
		return Result{
			Error:         lv.Error,
			EvaluatedRule: n,
		}
	}
	rv := n.rv.Eval(ctx)
	if !rv.Ok() {
		return Result{
			Error:         rv.Error,
			EvaluatedRule: n,
		}
	}

	rvArr, ok := rv.Value.([]any)
	if !ok {
		// the right value must be an array
		return Result{
			EvaluatedRule: n,
		}
	}

	// `FIELD in ARR` == `ARR contains FIELD`
	pass := compare(rvArr, op_CONTAINS, lv.Value)
	return Result{
		Value:         pass,
		EvaluatedRule: n,
	}
}

func (n *nodeIn) String() string {
	return n.lv.String() + " in " + n.rv.String()
}

func (n *nodeIn) ASTNode() ASTNode {
	return &ASTNodeOperator{
		Operator: "in",
		Left:     n.lv.ASTNode(),
		Right:    n.rv.ASTNode(),
	}
}
