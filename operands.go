package rulekit

import "context"

// isPresent reports whether res is complete with a value that is not null. A
// result that is not present is absent when it is not an error: missing, or
// null.
func isPresent(res Result) bool {
	return res.Complete() && res.Value != nil
}

// operand is an evaluated comparison operand. An array literal operand is
// evaluated element by element, so it can be incomplete while still having a
// Value: the elements that are present. literal is true for it.
type operand struct {
	Result
	literal bool
}

// arrayLiteral returns the array literal a lowered rule evaluates, if it is
// one, and the traced rule wrapping it.
func arrayLiteral(r Rule) (*tracedRule, *ArrayValue) {
	if traced, ok := r.(*tracedRule); ok {
		if arr, ok := traced.rule.(*ArrayValue); ok {
			return traced, arr
		}
	}
	return nil, nil
}

func evalOperand(ctx context.Context, input Input, opts Opts, r Rule) operand {
	if traced, arr := arrayLiteral(r); arr != nil {
		return operand{Result: evalArrayLiteral(ctx, input, opts, traced, arr, false), literal: true}
	}
	return operand{Result: r.Eval(ctx, input, opts)}
}

// evalArrayLiteral evaluates the elements of an array literal in order and
// does not stop at an incomplete element. The result's Value holds the
// elements that are complete (null ones included), MissingFields the fields
// of the others, and Error the first error, so the result is incomplete when
// any element is. With stopOnError, an element error stops the evaluation
// (later elements are pruned in the trace) and, with its own missing fields,
// is the whole result.
//
// The trace is the array's own: a value only when every element is complete.
func evalArrayLiteral(ctx context.Context, input Input, opts Opts, traced *tracedRule, arr *ArrayValue, stopOnError bool) Result {
	tracing := traceEnabled(opts)
	if arr.constant != nil && !tracing {
		return Result{Value: arr.constant}
	}
	var traces []*Trace
	if tracing {
		traces = make([]*Trace, 0, len(arr.vals))
	}
	vals := make([]any, 0, len(arr.vals))
	var (
		err     error
		missing []string
	)
	for i, item := range arr.vals {
		res := item.Eval(ctx, input, opts)
		if tracing {
			traces = append(traces, res.Trace)
		}
		if res.Ok() {
			vals = append(vals, res.Value)
			continue
		}
		if stopOnError && res.Error != nil {
			err, missing = res.Error, res.MissingFields
			if tracing {
				for _, rest := range arr.vals[i+1:] {
					traces = append(traces, prunedTrace(rest))
				}
			}
			break
		}
		if err == nil {
			err = res.Error
		}
		missing = unionUnique(missing, res.MissingFields)
	}

	out := Result{Value: vals, Error: err, MissingFields: missing}
	if tracing {
		shown := Result{Value: vals}
		if !out.Complete() {
			shown = Result{Error: err, MissingFields: missing}
		}
		out.Trace = &Trace{
			Node:          traced.node,
			Expr:          traced.expr,
			Value:         shown.Value,
			Error:         err,
			MissingFields: missing,
			Status:        traceStatus(shown),
			Active:        true,
			Children:      traces,
		}
	}
	return out
}

// isArrayLiteral reports whether a lowered rule evaluates an array literal.
func isArrayLiteral(r Rule) bool {
	_, arr := arrayLiteral(r)
	return arr != nil
}

// incompleteRight is the result of a comparison whose right operand rv is
// incomplete.
func incompleteRight(tracing bool, lv, rv Result) Result {
	return Result{
		Error:         rv.Error,
		MissingFields: rv.MissingFields,
		Trace:         traceIfEnabled(tracing, lv.Trace, rv.Trace),
	}
}

// evalComparison evaluates a comparison whose operand is an array literal
// (comparison nodes decide that when the rule is lowered and evaluate other
// comparisons themselves) and applies compare to the operand values. negated
// is true for `!=`, which passes when no element is equal; every other
// comparison passes when any element is.
//
// An incomplete plain operand is the result, and an incomplete left operand
// prunes the right one. An array literal operand is checked "any element":
// it is compared by its present elements. When some of its elements are
// incomplete and that comparison already decides the result (a pass, or a
// failure for `!=`), the result is final. Otherwise it is unknown or an error,
// carrying the missing fields of both operands.
func evalComparison(ctx context.Context, input Input, opts Opts, lvRule, rvRule Rule, negated bool, compare func(lv, rv any, tracing bool) (bool, Diagnostic)) Result {
	tracing := traceEnabled(opts)
	lv := evalOperand(ctx, input, opts, lvRule)
	if !lv.Ok() && !lv.literal {
		var trace *Trace
		if tracing {
			trace = combineTrace(lv.Trace, prunedTrace(rvRule))
		}
		return Result{
			Error:         lv.Error,
			MissingFields: lv.MissingFields,
			Trace:         trace,
		}
	}
	rv := evalOperand(ctx, input, opts, rvRule)
	trace := traceIfEnabled(tracing, lv.Trace, rv.Trace)
	undecided := Result{
		Error:         coalesceErrs(lv.Error, rv.Error),
		MissingFields: unionUnique(lv.MissingFields, rv.MissingFields),
		Trace:         trace,
	}
	if !rv.Ok() && !rv.literal {
		return undecided
	}

	pass, diagnostic := compare(lv.Value, rv.Value, tracing)
	if lv.Ok() && rv.Ok() {
		if diagnostic.Code != "" {
			trace = addTraceDiagnostic(trace, diagnostic)
		}
		return Result{Value: pass, Trace: trace}
	}
	if pass != negated {
		return Result{Value: pass, Trace: trace}
	}
	return undecided
}
