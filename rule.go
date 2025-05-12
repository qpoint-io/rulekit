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
	"unicode/utf8"
)

// Parse parses a rule expression and returns a Rule.
func Parse(str string) (Rule, error) {
	lexer := newLex([]byte(str))
	ok := ruleParse(lexer)

	if ok == 0 {
		return &rule{lexer.result}, nil
	}

	// If there's an error, create a more detailed error message
	line, col := getLineColumn(str, lexer.p)

	return nil, &ParseError{
		Line:       line,
		Column:     col,
		Message:    lexer.err,
		Input:      str,
		Suggestion: getSuggestion(lexer.err),
	}
}

func MustParse(str string) Rule {
	r, err := Parse(str)
	if err != nil {
		panic(err)
	}
	return r
}

type KV = map[string]any

type Rule interface {
	// Checks whether the input fields match the rule.
	Eval(KV) Result
	// String representation of the rule
	String() string
}

type rule struct {
	Rule
}

// Eval overrides the rule's Eval() method to wrap the returned EvalutedRule so we can override the String() method.
func (r *rule) Eval(fv KV) Result {
	res := r.Rule.Eval(fv)
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

// Ok returns true if the rule was able to evaluate.
func (r Result) Ok() bool {
	return r.Value != nil && r.Error == nil
}

// Pass returns true if the rule returns a non-zero value. This is usually used for boolean rules.
func (r Result) Pass() bool {
	return !isZero(r.Value)
}

// PassStrict returns true if the rule returns a non-zero value and all required fields are present.
func (r Result) PassStrict() bool {
	return r.Pass() && r.Ok()
}

// Fail returns true if the rule returns a zero value. This is usually used for boolean rules.
func (r Result) Fail() bool {
	return isZero(r.Value)
}

// FailStrict returns true if the rule returns a zero value and all required fields are present.
func (r Result) FailStrict() bool {
	return r.Fail() && r.Ok()
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
			"token_IP", `"ip"`,
			"token_IP_CIDR", `"cidr"`,
			"token_REGEX", `"regex"`,
			"token_FIELD", `"field name"`,
			"token_STRING", `"string"`,
			"token_HEX_STRING", `"hex"`,
			"token_ARRAY", `"array"`,
			"token_LBRACKET", `"["`,
			"token_RBRACKET", `"]"`,
			"token_LPAREN", `"("`,
			"token_RPAREN", `")"`,
			"token_FUNCTION", `"function or field name"`,
		)
		result += "\n" + replacer.Replace(e.Message)
	}

	if e.Suggestion != "" {
		result += "\nsuggestion: " + e.Suggestion
	}

	return result
}

// Helper function to get line and column from byte position
func getLineColumn(input string, pos int) (line, col int) {
	line = 1
	col = 1

	for i, ch := range input {
		if i >= pos {
			break
		}
		if ch == '\n' {
			line++
			col = 1
		} else {
			col += utf8.RuneLen(ch)
		}
	}
	return
}

// getSuggestion returns a helpful message based on the error
func getSuggestion(err string) string {
	switch {
	case strings.Contains(err, "parsing token_STRING"):
		return "string values must be properly quoted with matching quotes (e.g. \"hello\")"
	case strings.Contains(err, "parsing token_INT"):
		return "integer values must be valid integers without decimals (e.g. 42)"
	case strings.Contains(err, "parsing token_FLOAT"):
		return "floating-point numbers must be in the format 1.23"
	case strings.Contains(err, "parsing token_BOOL"):
		return "boolean values must be either 'true' or 'false' (case insensitive)"
	case strings.Contains(err, "parsing token_IP"):
		return "IP addresses must be in valid IPv4 (e.g. 192.168.1.1) or IPv6 format"
	case strings.Contains(err, "parsing token_IP_CIDR"):
		return "CIDR blocks must be in valid format (e.g. 192.168.1.0/24)"
	case strings.Contains(err, "parsing token_HEX_STRING"):
		return "hex strings must contain valid hex digits optionally separated by colons"
	case strings.Contains(err, "parsing token_REGEX"):
		return "regex patterns must be surrounded by / or | and contain valid regex syntax"
	case strings.Contains(err, "parsing token_FIELD"):
		return "field names must be valid identifiers (e.g. 'field_name' or 'field.name')"
	}
	return ""
}

func safeIndex[T any](slice []T, a, b int) []T {
	if a < 0 || b < 0 || a > b {
		return nil
	}
	if b > len(slice) {
		b = len(slice)
	}
	return slice[a:b]
}
