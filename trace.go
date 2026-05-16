package rulekit

// Trace explains how a rule evaluation reached its result.
type Trace struct {
	Node     ASTNode
	Expr     string
	Value    any
	Error    error
	Active   bool
	Pruned   bool
	Children []*Trace
}

type tracedRule struct {
	node astNode
	rule Rule
}

func withTrace(node astNode, rule Rule) Rule {
	if rule == nil {
		return nil
	}
	return &tracedRule{node: node, rule: rule}
}

func (r *tracedRule) Eval(ctx *Ctx) Result {
	res := r.rule.Eval(ctx)
	if traceEnabled(ctx) {
		node := astToPublic(r.node)
		res.Trace = &Trace{
			Node:     node,
			Expr:     traceExpr(node, r.rule),
			Value:    res.Value,
			Error:    res.Error,
			Active:   true,
			Children: traceChildren(res.Trace),
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

func traceEnabled(ctx *Ctx) bool {
	return ctx != nil && ctx.Trace
}

func traceChildren(trace *Trace) []*Trace {
	if trace == nil {
		return nil
	}
	return trace.Children
}

func prunedTrace(rule Rule) *Trace {
	if rule == nil {
		return nil
	}
	if traced, ok := rule.(*tracedRule); ok {
		node := astToPublic(traced.node)
		return &Trace{Node: node, Expr: traceExpr(node, traced.rule), Pruned: true}
	}
	return &Trace{Expr: rule.String(), Pruned: true}
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
