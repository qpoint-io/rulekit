package rulekit

import "fmt"

// Plan is an optional compiled runtime representation for repeated evaluation.
type Plan struct {
	rule Rule
}

// CompilePlan lowers an AST into a reusable runtime plan.
func CompilePlan(ast *AST) (*Plan, error) {
	rule, err := Compile(ast)
	if err != nil {
		return nil, err
	}
	return &Plan{rule: rule}, nil
}

// ParsePlan parses and compiles an expression into a runtime plan.
func ParsePlan(expr string) (*Plan, error) {
	ast, err := ParseAST(expr)
	if err != nil {
		return nil, err
	}
	return CompilePlan(ast)
}

func (p *Plan) Eval(ctx *Ctx) Result {
	if p == nil || p.rule == nil {
		return Result{Error: fmt.Errorf("plan must not be nil")}
	}
	return p.rule.Eval(ctx)
}

func (p *Plan) String() string {
	if p == nil || p.rule == nil {
		return "<empty>"
	}
	return p.rule.String()
}

func (p *Plan) Rule() Rule {
	if p == nil {
		return nil
	}
	return p.rule
}
