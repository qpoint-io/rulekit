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
	if !r.Ok() {
		return Result{
			Error:         r.Error,
			MissingFields: r.MissingFields,
			Trace:         traceIfEnabled(traceEnabled(opts), r.Trace),
		}
	}

	return Result{
		Value: !isZero(r.Value),
		Trace: traceIfEnabled(traceEnabled(opts), r.Trace),
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
}

func (n *nodeMatch) Eval(ctx context.Context, input Input, opts Opts) Result {
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
		return Result{
			Error:         rv.Error,
			MissingFields: rv.MissingFields,
			Trace:         traceIfEnabled(tracing, lv.Trace, rv.Trace),
		}
	}

	return Result{
		Value: n.apply(lv.Value, rv.Value),
		Trace: traceIfEnabled(tracing, lv.Trace, rv.Trace),
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

func (n *nodeMatch) Print(PrintMode) string {
	return n.String()
}

// Comparison node
type nodeCompare struct {
	lv Rule
	op int // op_EQ, NE, GT, GE, LT, LE, CONTAINS
	rv Rule
}

func (n *nodeCompare) Eval(ctx context.Context, input Input, opts Opts) Result {
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
		return Result{
			Error:         rv.Error,
			MissingFields: rv.MissingFields,
			Trace:         traceIfEnabled(tracing, lv.Trace, rv.Trace),
		}
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

func (n *nodeCompare) String() string {
	return n.lv.String() + " " + operatorToString(n.op) + " " + n.rv.String()
}

func (n *nodeCompare) Print(PrintMode) string {
	return n.String()
}

// TEST_IN
type nodeIn struct {
	lv Rule
	rv Rule
}

func (n *nodeIn) Eval(ctx context.Context, input Input, opts Opts) Result {
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
		return Result{
			Error:         rv.Error,
			MissingFields: rv.MissingFields,
			Trace:         traceIfEnabled(tracing, lv.Trace, rv.Trace),
		}
	}

	rvArr, ok := rv.Value.([]any)
	if !ok {
		// the right value must be an array
		trace := traceIfEnabled(tracing, lv.Trace, rv.Trace)
		if tracing {
			trace = addTraceDiagnostic(trace, newComparisonDiagnostic(compareDiagnosticInvalidShape, lv.Value, op_IN, rv.Value))
		}
		return Result{
			Trace: trace,
		}
	}

	// `FIELD in ARR` == `ARR contains FIELD`
	outcome := compareDetailed(rvArr, op_CONTAINS, lv.Value)
	trace := traceIfEnabled(tracing, lv.Trace, rv.Trace)
	if tracing && outcome.diagnostic != compareDiagnosticNone {
		trace = addTraceDiagnostic(trace, newComparisonDiagnostic(outcome.diagnostic, rvArr, op_CONTAINS, lv.Value))
	}
	return Result{
		Value: outcome.pass,
		Trace: trace,
	}
}

func (n *nodeIn) String() string {
	return n.lv.String() + " in " + n.rv.String()
}

func (n *nodeIn) Print(PrintMode) string {
	return n.String()
}
