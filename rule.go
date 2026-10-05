// Package rulekit parses and evaluates rule expressions such as
//
//	domain matches /example\.com$/ and port == 8080
//
// against input values. See the README for the language reference:
// https://github.com/qpoint-io/rulekit#readme
package rulekit

import (
	"context"
	"fmt"
	"strings"
)

// Parse parses a rule expression and returns a Rule.
func Parse(str string) (Rule, error) {
	ast, err := ParseAST(str)
	if err != nil {
		return nil, err
	}
	return Compile(ast)
}

func MustParse(str string) Rule {
	r, err := Parse(str)
	if err != nil {
		panic(err)
	}
	return r
}

type KV = map[string]any

type Opts struct {
	Trace     bool
	Macros    MacroSet
	Functions map[string]*Function
}

func (o Opts) Validate() error {
	for name, fn := range o.Functions {
		if _, ok := StdlibFuncs[name]; ok {
			return fmt.Errorf("function %q: name conflicts with a stdlib function", name)
		}
		if fn == nil {
			return fmt.Errorf("function %q: must not be nil", name)
		}
	}
	for name, macro := range o.Macros {
		if _, ok := StdlibFuncs[name]; ok {
			return fmt.Errorf("macro %q: name conflicts with a stdlib function", name)
		}
		if _, ok := o.Functions[name]; ok {
			return fmt.Errorf("macro %q: name conflicts with a custom function", name)
		}
		if macro == nil || macro.Rule == nil {
			return fmt.Errorf("macro %q: must not be nil", name)
		}
	}

	return nil
}

type Rule interface {
	// Evaluates the rule with the context and input.
	Eval(context.Context, Input, Opts) Result
	// Print prints the rule with the requested mode.
	Print(PrintMode) string
	// String representation of the rule
	String() string
}

type RuleFunc func(context.Context, Input, Opts) Result

func (f RuleFunc) Eval(ctx context.Context, input Input, opts Opts) Result {
	return f(ctx, input, opts)
}

func (f RuleFunc) String() string {
	return "<fn>"
}

func (f RuleFunc) Print(PrintMode) string {
	return f.String()
}

type rule struct {
	Rule
	ast *AST
}

// Eval evaluates the compiled rule. A nil context is treated as context.Background().
func (r *rule) Eval(ctx context.Context, input Input, opts Opts) Result {
	if ctx == nil {
		ctx = context.Background()
	}
	if err := opts.Validate(); err != nil {
		return Result{Error: err}
	}

	return r.Rule.Eval(ctx, input, opts)
}

func (r *rule) Print(mode PrintMode) string {
	if r.Rule == nil {
		return "<empty>"
	}
	if r.ast != nil {
		return Format(r.ast, mode)
	}
	return r.String()
}

// String overrides the rule's String() method to remove the parentheses.
// This is only used on the root node.
func (r *rule) String() string {
	if r.Rule == nil {
		return "<empty>"
	}
	if r.ast != nil {
		return r.ast.String()
	}
	s := r.Rule.String()
	if len(s) > 0 && s[0] == '(' {
		return strings.TrimSuffix(s[1:], ")")
	}
	return s
}

type Result struct {
	Value         any
	Error         error
	MissingFields []string
	Trace         *Trace
}

// Ok returns true if the rule evaluated completely without errors or missing fields.
func (r Result) Ok() bool {
	return r.Complete()
}

// Complete returns true if the rule evaluated without errors or missing fields.
func (r Result) Complete() bool {
	return r.Error == nil && len(r.MissingFields) == 0
}

// Unknown returns true if evaluation needs more input but did not otherwise fail.
func (r Result) Unknown() bool {
	return r.Error == nil && len(r.MissingFields) > 0
}

// Pass returns true if the result is ok with a non-zero value. This is usually used for boolean rules.
func (r Result) Pass() bool {
	return r.Complete() && !isZero(r.Value)
}

// Fail returns true if the rule is ok and returns a zero value. This is usually used for boolean rules.
func (r Result) Fail() bool {
	return r.Complete() && isZero(r.Value)
}

type ParseError struct {
	Line       int
	Column     int
	Message    string
	Input      string
	Suggestion string
}

func (e *ParseError) Error() string {
	// Get the line containing the error
	lines := strings.Split(e.Input, "\n")
	var errorLine string
	if e.Line-1 < len(lines) {
		errorLine = lines[e.Line-1]
	}

	result := fmt.Sprintf("syntax error at line %d:%d:\n%s", e.Line, e.Column, errorLine)

	// Add pointer to the error location
	if errorLine != "" {
		pointer := strings.Repeat(" ", e.Column-1) + "^"
		result += "\n" + pointer
	}

	if e.Message != "" {
		result += "\n" + e.Message
	}

	if e.Suggestion != "" {
		result += "\nsuggestion: " + e.Suggestion
	}

	return result
}
