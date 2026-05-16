package rulekit

import "context"

type DiagnosticCode string

const (
	DiagnosticComparisonIncomparable        DiagnosticCode = "comparison_incomparable"
	DiagnosticComparisonInvalidShape        DiagnosticCode = "comparison_invalid_shape"
	DiagnosticComparisonUnsupportedOperator DiagnosticCode = "comparison_unsupported_operator"
)

// Diagnostic provides optional evaluation details for trace consumers.
type Diagnostic struct {
	Code      DiagnosticCode
	Message   string
	LeftType  string
	Operator  string
	RightType string
}

type TraceStatus string

const (
	TraceUnknown TraceStatus = "unknown"
	TracePassed  TraceStatus = "passed"
	TraceFailed  TraceStatus = "failed"
	TraceMissing TraceStatus = "missing"
	TraceError   TraceStatus = "error"
	TracePruned  TraceStatus = "pruned"
)

// Trace explains how a rule evaluation reached its result.
type Trace struct {
	Node          ASTNode
	Expr          string
	Value         any
	Error         error
	MissingFields []string
	Diagnostics   []Diagnostic
	Status        TraceStatus
	Active        bool
	Pruned        bool
	Children      []*Trace
}

type tracedRule struct {
	node ASTNode
	expr string
	rule Rule
}

func withTrace(node astNode, rule Rule) Rule {
	if rule == nil {
		return nil
	}
	public := astToPublic(node)
	return &tracedRule{node: public, expr: traceExpr(public, rule), rule: rule}
}

func (r *tracedRule) Eval(ctx context.Context, input Input, opts Opts) Result {
	res := r.rule.Eval(ctx, input, opts)
	if traceEnabled(opts) {
		res.Trace = &Trace{
			Node:          r.node,
			Expr:          r.expr,
			Value:         res.Value,
			Error:         res.Error,
			MissingFields: res.MissingFields,
			Diagnostics:   traceDiagnostics(res.Trace),
			Status:        traceStatus(res),
			Active:        true,
			Children:      traceChildren(res.Trace),
		}
	}
	return res
}

func (r *tracedRule) Print(mode PrintMode) string {
	return r.rule.Print(mode)
}

func (r *tracedRule) String() string {
	return r.rule.String()
}

func unwrapTracedRule(rule Rule) Rule {
	for {
		traced, ok := rule.(*tracedRule)
		if !ok {
			return rule
		}
		rule = traced.rule
	}
}

func traceEnabled(opts Opts) bool {
	return opts.Trace
}

func traceChildren(trace *Trace) []*Trace {
	if trace == nil {
		return nil
	}
	return trace.Children
}

func traceDiagnostics(trace *Trace) []Diagnostic {
	if trace == nil {
		return nil
	}
	return trace.Diagnostics
}

func addTraceDiagnostic(trace *Trace, diagnostic Diagnostic) *Trace {
	if trace == nil {
		trace = &Trace{}
	}
	trace.Diagnostics = append(trace.Diagnostics, diagnostic)
	return trace
}

func prunedTrace(rule Rule) *Trace {
	if rule == nil {
		return nil
	}
	if traced, ok := rule.(*tracedRule); ok {
		return &Trace{Node: traced.node, Expr: traced.expr, Status: TracePruned, Pruned: true}
	}
	return &Trace{Expr: rule.String(), Status: TracePruned, Pruned: true}
}

func traceExpr(node ASTNode, rule Rule) string {
	if node != nil {
		return node.String()
	}
	return rule.String()
}

func combineTrace(children ...*Trace) *Trace {
	var count int
	for _, child := range children {
		if child != nil {
			count++
		}
	}
	if count == 0 {
		return nil
	}
	trace := &Trace{Children: make([]*Trace, 0, count)}
	for _, child := range children {
		if child != nil {
			trace.Children = append(trace.Children, child)
		}
	}
	return trace
}

func traceIfEnabled(enabled bool, children ...*Trace) *Trace {
	if !enabled {
		return nil
	}
	return combineTrace(children...)
}

func traceStatus(res Result) TraceStatus {
	switch {
	case res.Error != nil:
		return TraceError
	case len(res.MissingFields) > 0:
		return TraceMissing
	case res.Pass():
		return TracePassed
	case res.Fail():
		return TraceFailed
	default:
		return TraceUnknown
	}
}
