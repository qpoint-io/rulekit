package rulekit

import (
	"context"
	"fmt"
	"regexp"
)

// AND
type nodeAnd struct {
	left  Rule
	right Rule
}

func (n *nodeAnd) Eval(ctx context.Context, input Input, opts Opts) Result {
	tracing := traceEnabled(opts)
	// if either node fails, return only that node
	rleft := n.left.Eval(ctx, input, opts)
	if rleft.Fail() {
		if tracing {
			rleft.Trace = combineTrace(rleft.Trace, prunedTrace(n.right))
		}
		return rleft
	}

	rright := n.right.Eval(ctx, input, opts)
	if rright.Fail() {
		if tracing {
			rright.Trace = combineTrace(rleft.Trace, rright.Trace)
		}
		return rright
	}

	// if only one node is not ok, return it (keeping both operand traces)
	if rleft.Ok() && !rright.Ok() {
		rright.Trace = traceIfEnabled(tracing, rleft.Trace, rright.Trace)
		return rright
	} else if !rleft.Ok() && rright.Ok() {
		rleft.Trace = traceIfEnabled(tracing, rleft.Trace, rright.Trace)
		return rleft
	}

	// at this point either both nodes are ok or both are not ok. When both are
	// ok the left passed and the right was evaluated, so like JS the right
	// operand's value is the result.
	var value any
	if rleft.Ok() && rright.Ok() {
		value = rright.Value
	}

	return Result{
		Value:         value,
		Error:         coalesceErrs(rleft.Error, rright.Error),
		MissingFields: coalesceMissingFields(rleft.MissingFields, rright.MissingFields),
		Trace:         traceIfEnabled(tracing, rleft.Trace, rright.Trace),
	}
}

func (n *nodeAnd) String() string {
	return fmt.Sprintf("(%s and %s)", n.left.String(), n.right.String())
}

func (n *nodeAnd) Print(PrintMode) string {
	return n.String()
}

// OR
type nodeOr struct {
	left  Rule
	right Rule
}

func (n *nodeOr) Eval(ctx context.Context, input Input, opts Opts) Result {
	tracing := traceEnabled(opts)
	// if either node passes, return only that node
	rleft := n.left.Eval(ctx, input, opts)
	if rleft.Pass() {
		if tracing {
			rleft.Trace = combineTrace(rleft.Trace, prunedTrace(n.right))
		}
		return rleft
	}

	rright := n.right.Eval(ctx, input, opts)
	if rright.Pass() {
		if tracing {
			rright.Trace = combineTrace(rleft.Trace, rright.Trace)
		}
		return rright
	}

	// if only one node is not ok, return it (keeping both operand traces)
	if rleft.Ok() && !rright.Ok() {
		rright.Trace = traceIfEnabled(tracing, rleft.Trace, rright.Trace)
		return rright
	} else if !rleft.Ok() && rright.Ok() {
		rleft.Trace = traceIfEnabled(tracing, rleft.Trace, rright.Trace)
		return rleft
	}

	// at this point either both nodes are ok or both are not ok. When both are
	// ok the left failed and the right was evaluated, so like JS the right
	// operand's value is the result.
	var value any
	if rleft.Ok() && rright.Ok() {
		value = rright.Value
	}

	return Result{
		Value:         value,
		Error:         coalesceErrs(rleft.Error, rright.Error),
		MissingFields: coalesceMissingFields(rleft.MissingFields, rright.MissingFields),
		Trace:         traceIfEnabled(tracing, rleft.Trace, rright.Trace),
	}
}

func (n *nodeOr) String() string {
	return fmt.Sprintf("(%s or %s)", n.left.String(), n.right.String())
}

func (n *nodeOr) Print(PrintMode) string {
	return n.String()
}

// NOT
type nodeNot struct {
	right Rule
}

func (n *nodeNot) Eval(ctx context.Context, input Input, opts Opts) Result {
	if n.right == nil {
		return Result{}
	}

	r := n.right.Eval(ctx, input, opts)
	var trace *Trace
	if traceEnabled(opts) {
		// A negated operator (`a not in b`) has no traced node between it and
		// its operands, so its operand traces become its children directly.
		if trace = r.Trace; trace == nil || trace.Node != nil {
			trace = combineTrace(r.Trace)
		}
	}
	if !r.Ok() {
		return Result{
			Error:         r.Error,
			MissingFields: r.MissingFields,
			Trace:         trace,
		}
	}

	return Result{
		Value: isZero(r.Value),
		Trace: trace,
	}
}

func (n *nodeNot) String() string {
	right := unwrapTracedRule(n.right)
	if nn, ok := right.(*nodeCompare); ok {
		if nn.op == op_EQ {
			// special formatting for !=
			return nn.lv.String() + " != " + nn.rv.String()
		} else if nn.op == op_CONTAINS {
			// special formatting for field not contains "item"
			return nn.lv.String() + " not contains " + nn.rv.String()
		}
	} else if nn, ok := right.(FieldValue); ok {
		// special formatting for !FIELD (no space between ! and field)
		return "!" + nn.String()
	} else if nn, ok := right.(*nodeMatch); ok {
		// special formatting for field not =~ /pattern/
		return nn.lv.String() + " not =~ " + nn.rv.String()
	} else if nn, ok := right.(*nodeIn); ok {
		// special formatting for field not in [1, "str", 3]
		return nn.lv.String() + " not in " + nn.rv.String()
	}

	return "not (" + right.String() + ")"
}

func (n *nodeNot) Print(PrintMode) string {
	return n.String()
}

// TEST_MATCHES
type nodeMatch struct {
	lv Rule
	rv Rule
	// literal is true when an operand is an array literal.
	literal bool
}

func (n *nodeMatch) Eval(ctx context.Context, input Input, opts Opts) Result {
	if n.literal {
		return evalComparison(ctx, input, opts, n.lv, n.rv, false, n.compare)
	}
	tracing := traceEnabled(opts)
	lv := n.lv.Eval(ctx, input, opts)
	if !lv.Ok() {
		var trace *Trace
		if tracing {
			trace = combineTrace(lv.Trace, prunedTrace(n.rv))
		}
		return Result{
			Error:         lv.Error,
			MissingFields: lv.MissingFields,
			Trace:         trace,
		}
	}
	rv := n.rv.Eval(ctx, input, opts)
	if !rv.Ok() {
		return incompleteRight(tracing, lv, rv)
	}

	return Result{
		Value: n.apply(lv.Value, rv.Value),
		Trace: traceIfEnabled(tracing, lv.Trace, rv.Trace),
	}
}

func (n *nodeMatch) compare(lv, rv any, _ bool) (bool, Diagnostic) {
	return n.apply(lv, rv), Diagnostic{}
}

func (n *nodeMatch) apply(lv any, rv any) bool {
	r, ok := rv.(*regexp.Regexp)
	if !ok || r == nil {
		return false
	}

	switch val := lv.(type) {
	case []string:
		for _, s := range val {
			if r.MatchString(s) {
				return true
			}
		}
	case []any:
		for _, el := range val {
			if s, ok := stringable(el); ok && r.MatchString(s) {
				return true
			}
		}
	default:
		if s, ok := stringable(lv); ok {
			return r.MatchString(s)
		}
	}
	return false
}

func (n *nodeMatch) String() string {
	return operandString(n.lv) + " =~ " + operandString(n.rv)
}

// operandString prints a comparison operand, parenthesizing negations because
// not binds looser than comparisons.
func operandString(r Rule) string {
	if _, ok := unwrapTracedRule(r).(*nodeNot); ok {
		return "(" + r.String() + ")"
	}
	return r.String()
}

func (n *nodeMatch) Print(PrintMode) string {
	return n.String()
}

// Comparison node
type nodeCompare struct {
	lv Rule
	op int // op_EQ, NE, GT, GE, LT, LE, CONTAINS
	rv Rule
	// literal is true when an operand is an array literal.
	literal bool
}

func (n *nodeCompare) Eval(ctx context.Context, input Input, opts Opts) Result {
	if n.literal {
		return evalComparison(ctx, input, opts, n.lv, n.rv, n.op == op_NE, n.compare)
	}
	tracing := traceEnabled(opts)
	lv := n.lv.Eval(ctx, input, opts)
	if !lv.Ok() {
		var trace *Trace
		if tracing {
			trace = combineTrace(lv.Trace, prunedTrace(n.rv))
		}
		return Result{
			Error:         lv.Error,
			MissingFields: lv.MissingFields,
			Trace:         trace,
		}
	}
	rv := n.rv.Eval(ctx, input, opts)
	if !rv.Ok() {
		return incompleteRight(tracing, lv, rv)
	}

	outcome := compareDetailed(lv.Value, n.op, rv.Value)
	trace := traceIfEnabled(tracing, lv.Trace, rv.Trace)
	if tracing && outcome.diagnostic != compareDiagnosticNone {
		trace = addTraceDiagnostic(trace, newComparisonDiagnostic(outcome.diagnostic, lv.Value, n.op, rv.Value))
	}
	return Result{
		Value: outcome.pass,
		Trace: trace,
	}
}

func (n *nodeCompare) compare(lv, rv any, tracing bool) (bool, Diagnostic) {
	outcome := compareDetailed(lv, n.op, rv)
	var diagnostic Diagnostic
	if tracing && outcome.diagnostic != compareDiagnosticNone {
		diagnostic = newComparisonDiagnostic(outcome.diagnostic, lv, n.op, rv)
	}
	return outcome.pass, diagnostic
}

func (n *nodeCompare) String() string {
	return operandString(n.lv) + " " + operatorToString(n.op) + " " + operandString(n.rv)
}

func (n *nodeCompare) Print(PrintMode) string {
	return n.String()
}

// TEST_IN
type nodeIn struct {
	lv Rule
	rv Rule
	// literal is true when an operand is an array literal.
	literal bool
}

func (n *nodeIn) Eval(ctx context.Context, input Input, opts Opts) Result {
	if n.literal {
		return evalComparison(ctx, input, opts, n.lv, n.rv, false, n.compare)
	}
	tracing := traceEnabled(opts)
	lv := n.lv.Eval(ctx, input, opts)
	if !lv.Ok() {
		var trace *Trace
		if tracing {
			trace = combineTrace(lv.Trace, prunedTrace(n.rv))
		}
		return Result{
			Error:         lv.Error,
			MissingFields: lv.MissingFields,
			Trace:         trace,
		}
	}
	rv := n.rv.Eval(ctx, input, opts)
	if !rv.Ok() {
		return incompleteRight(tracing, lv, rv)
	}

	pass, diagnostic := n.compare(lv.Value, rv.Value, tracing)
	trace := traceIfEnabled(tracing, lv.Trace, rv.Trace)
	if diagnostic.Code != "" {
		trace = addTraceDiagnostic(trace, diagnostic)
	}
	return Result{
		Value: pass,
		Trace: trace,
	}
}

func (n *nodeIn) compare(lv, rv any, tracing bool) (bool, Diagnostic) {
	// A right side that is not a list, such as a field holding a string,
	// contains nothing.
	rvArr, ok := asArray(rv)
	if !ok {
		var diagnostic Diagnostic
		if tracing {
			diagnostic = newComparisonDiagnostic(compareDiagnosticInvalidShape, lv, op_IN, rv)
		}
		return false, diagnostic
	}

	// `FIELD in ARR` == `ARR contains FIELD`, except a regex element matches
	// FIELD like `FIELD == /re/`. A list-valued FIELD is in ARR when ANY of its
	// elements is.
	outcome, ok := compareAnyElem(lv, func(el any) compareOutcome {
		return compareIn(el, rvArr)
	})
	if !ok {
		outcome = compareIn(lv, rvArr)
	}
	var diagnostic Diagnostic
	if tracing && outcome.diagnostic != compareDiagnosticNone {
		diagnostic = newComparisonDiagnostic(outcome.diagnostic, rvArr, op_CONTAINS, lv)
	}
	return outcome.pass, diagnostic
}

// compareIn reports whether x is an element of list: `list contains x`, with
// regex elements compared as `x == /re/` so `"ab" in [/a/]` matches.
func compareIn(x any, list []any) compareOutcome {
	switch x.(type) {
	case []any, []string:
		// the contains operator does not support arrays on the right side.
		return invalidShape()
	}
	return compareSliceDetailed(list, op_EQ, func(item any, op int) compareOutcome {
		if re, ok := item.(*regexp.Regexp); ok {
			return compareDetailed(x, op, re)
		}
		return compareDetailed(item, op, x)
	})
}

func (n *nodeIn) String() string {
	return operandString(n.lv) + " in " + operandString(n.rv)
}

func (n *nodeIn) Print(PrintMode) string {
	return n.String()
}

// COALESCE
type nodeCoalesce struct {
	left  Rule
	right Rule
}

// Eval returns the left result when it is an error or has a value that is not
// null, and otherwise (missing or null) the right result unchanged. The left
// result's missing fields are dropped.
func (n *nodeCoalesce) Eval(ctx context.Context, input Input, opts Opts) Result {
	tracing := traceEnabled(opts)
	rleft := n.left.Eval(ctx, input, opts)
	if isPresent(rleft) || rleft.Error != nil {
		if tracing {
			rleft.Trace = combineTrace(rleft.Trace, prunedTrace(n.right))
		}
		return rleft
	}

	rright := n.right.Eval(ctx, input, opts)
	if tracing {
		rright.Trace = combineTrace(rleft.Trace, rright.Trace)
	}
	return rright
}

func (n *nodeCoalesce) String() string {
	return fmt.Sprintf("(%s ?? %s)", n.left.String(), n.right.String())
}

func (n *nodeCoalesce) Print(PrintMode) string {
	return n.String()
}
