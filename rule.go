package rulekit

//go:generate bash gen.sh

/*

	This package implements an expression-based rules engine.

	An expression is evaluated against a KV map of values returning a true/false result.
		For example, with the expression:
			domain matches /example\.com$/

		And the following KV maps:
			map[string]any{"domain": "example.com"} -> true
			map[string]any{"domain": "qpoint.io"}   -> false

		In this example,
			domain				is a FIELD,
			matches				is the OPERATOR,
			/example\.com$/		is the VALUE.

	A FIELD or VALUE may appear on either side of an operator.
		For example, all of the following expressions are valid:
			- port == 8080
			- 8080 == port
			- src.port == dst.port
			- 500 > 2

	A FIELD or VALUE on its own without an operator will check if the field contains a non-zero value.
		For example: `bool_field && string_field`

	Supported operators:
		== (eq), != (ne), > (gt), >= (ge), < (lt), <= (le), contains, matches, in
		or (||), and (&&), not (!)
		() parentheses for grouping

	Supported types:

		bool: VALUE, FIELD
			e.g. true

			valid values: true, false

		number: VALUE, FIELD
			e.g. 8080, 1.35

			numbers are parsed as either int64 or uint64 if out of range for int64
			floats are parsed as float64

			Go type: int64, uint64, float64

		string: VALUE, FIELD
			e.g. "domain.com"

			a double-quoted string. quotes may be escaped with a backslash, e.g. "a string \"with\" quotes"
			any quoted value is parsed as a string

		IP address: VALUE, FIELD
			e.g. 192.168.1.1
			e.g. 2001:db8:3333:4444:cccc:dddd:eeee:ffff
			e.g. 2001:db8:3333:4444:5555:6666:1.2.3.4

			An IPv4, IPv6 or an IPv6 dual address.

			Go type: net.IP

		CIDR: VALUE
			e.g. 192.168.1.0/24
			e.g. 2001:db8:3333:4444:cccc:dddd:eeee:ffff/64

			An IPv4 or IPv6 CIDR block.

			Go type: *net.IPNet

		Hexadecimal string: VALUE, FIELD
			e.g. 12:34:56:78:ab (MAC address)
			e.g. 504f5354 (hex string "POST")

			a hexadecimal string, optionally separated by colons.

			Go type:
				- FIELD: []byte
				- VALUE: rule.HexString (hexstring.go)

		Regex: VALUE
			e.g. /example\.com$/

			a Go-style regular expression. Must be surrounded by forward slashes. May not be quoted with double quotes (otherwise it will be parsed as a string).

			Go type: *regexp.Regexp

*/

import (
	"fmt"
	"strings"
)

// Parse parses a rule expression and returns a Rule.
func Parse(str string) (Rule, error) {
	r, err := parseRule(str)
	if err != nil {
		return nil, err
	}
	return &rule{r}, nil
}

func MustParse(str string) Rule {
	r, err := Parse(str)
	if err != nil {
		panic(err)
	}
	return r
}

type KV = map[string]any

type Ctx struct {
	KV        KV
	Macros    map[string]Rule
	Functions map[string]*Function
}

func (c *Ctx) Eval(r Rule) Result {
	return r.Eval(c)
}

func (c *Ctx) Validate() error {
	for name, fn := range c.Functions {
		if _, ok := StdlibFuncs[name]; ok {
			return fmt.Errorf("function %q: name conflicts with a stdlib function", name)
		}
		if fn == nil {
			return fmt.Errorf("function %q: must not be nil", name)
		}
	}
	for name, macro := range c.Macros {
		if _, ok := StdlibFuncs[name]; ok {
			return fmt.Errorf("macro %q: name conflicts with a stdlib function", name)
		}
		if _, ok := c.Functions[name]; ok {
			return fmt.Errorf("macro %q: name conflicts with a custom function", name)
		}
		if macro == nil {
			return fmt.Errorf("macro %q: must not be nil", name)
		}
	}

	return nil
}

type Rule interface {
	// Evaluates the rule with the context
	Eval(*Ctx) Result
	// String representation of the rule
	String() string
}

type RuleFunc func(*Ctx) Result

func (f RuleFunc) Eval(ctx *Ctx) Result {
	return f(ctx)
}

func (f RuleFunc) String() string {
	return "<fn>"
}

type rule struct {
	Rule
}

// Eval overrides the rule's Eval() method to wrap the returned EvalutedRule so we can override the String() method.
func (r *rule) Eval(ctx *Ctx) Result {
	if err := ctx.Validate(); err != nil {
		return Result{Error: err}
	}

	res := r.Rule.Eval(ctx)
	res.EvaluatedRule = &rule{Rule: res.EvaluatedRule}
	return res
}

// String overrides the rule's String() method to remove the parentheses.
// This is only used on the root node.
func (r *rule) String() string {
	if r.Rule == nil {
		return "<empty>"
	}
	s := r.Rule.String()
	if len(s) > 0 && s[0] == '(' {
		return strings.TrimSuffix(s[1:], ")")
	}
	return s
}

type Result struct {
	Value         any
	EvaluatedRule Rule
	Error         error
}

// Ok returns true if the rule was able to evaluate without error.
func (r Result) Ok() bool {
	return r.Error == nil
}

// Pass returns true if the result is ok with a non-zero value. This is usually used for boolean rules.
func (r Result) Pass() bool {
	return r.Ok() && !isZero(r.Value)
}

// Fail returns true if the rule is ok and returns a zero value. This is usually used for boolean rules.
func (r Result) Fail() bool {
	return r.Ok() && isZero(r.Value)
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
		replacer := strings.NewReplacer(
			"token_ERROR", `symbol`,
			"token_LPAREN", `"("`,
			"token_RPAREN", `")"`,
			"op_NOT", `"!"`,
			"op_AND", `"&&"`,
			"op_OR", `"||"`,
			"op_EQ", `"=="`,
			"op_NE", `"!="`,
			"op_GT", `">"`,
			"op_GE", `">="`,
			"op_LT", `"<"`,
			"op_LE", `"<="`,
			"op_CONTAINS", `"contains"`,
			"op_MATCHES", `"=~"`,
			"token_INT", `"integer"`,
			"token_FLOAT", `"float"`,
			"token_BOOL", `"boolean"`,
			"token_IP_CIDR", `"cidr"`,
			"token_IP", `"ip"`,
			"token_REGEX", `"regex"`,
			"token_FIELD", `"field name"`,
			"token_STRING", `"string"`,
			"token_HEX_STRING", `"hex"`,
			"token_ARRAY", `"array"`,
			"token_LBRACKET", `"["`,
			"token_RBRACKET", `"]"`,
			"token_DOT", `"."`,
			"token_LPAREN", `"("`,
			"token_RPAREN", `")"`,
			"token_FUNCTION", `"function or field identifier"`,
		)
		result += "\n" + replacer.Replace(e.Message)
	}

	if e.Suggestion != "" {
		result += "\nsuggestion: " + e.Suggestion
	}

	return result
}
