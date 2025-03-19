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

	Operations always follow the syntax FIELD OPERATOR VALUE.
		In the example expression:
			FIELD:     domain
			OPERATOR:  matches
			VALUE:     /example\.com$/

	Additionally, a FIELD on its own without an operator will check if the field contains a non-zero value.

	Supported operators:
		== (eq), != (ne), > (gt), >= (ge), < (lt), <= (le), contains, matches
		or (||), and (&&), not (!)
		() parentheses for grouping

	Supported types:

		bool: VALUE, FIELD
			e.g. true

			valid values: true, false

		number: VALUE, FIELD
			e.g. 8080

			numbers are parsed as either int64 or uint64 if out of range for int64
			floats are not supported

			Go type: int64 or uint64

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

	"github.com/qpoint-io/rulekit/set"
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
	return strings.TrimSuffix(strings.TrimPrefix(r.Rule.String(), "("), ")")
}

type Result struct {
	Pass          bool
	MissingFields set.Set[string]
	EvaluatedRule Rule
}

// Pass returns true if the rule passes and all required fields are present.
func (r Result) PassStrict() bool {
	return r.Pass && r.Strict()
}

// FailStrict returns true if the rule fails and all required fields are present.
func (r Result) FailStrict() bool {
	return !r.Pass && r.Strict()
}

// Strict returns true if the rule fails and all required fields are present.
func (r Result) Strict() bool {
	return len(r.MissingFields) == 0
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
			"token_ERROR", `"symbol"`,
			"token_LPAREN", `"("`,
			"token_RPAREN", `")"`,
			"token_TEST_NOT", `"!"`,
			"token_TEST_AND", `"&&"`,
			"token_TEST_OR", `"||"`,
			"token_TEST_EQ", `"=="`,
			"token_TEST_NE", `"!="`,
			"token_TEST_GT", `">"`,
			"token_TEST_GE", `">="`,
			"token_TEST_LT", `"<"`,
			"token_TEST_LE", `"<="`,
			"token_TEST_CONTAINS", `"contains"`,
			"token_TEST_MATCHES", `"=~"`,
			"token_INT", `"integer"`,
			"token_FLOAT", `"float"`,
			"token_BOOL", `"boolean"`,
			"token_IP", `"ip"`,
			"token_IP_CIDR", `"cidr"`,
			"token_REGEX", `"regex"`,
			"token_FIELD", `"field name"`,
			"token_STRING", `"string"`,
			"token_HEX_STRING", `"hex"`,
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
	case strings.Contains(err, "parsing string"):
		return "string values must be properly quoted with matching quotes"
	case strings.Contains(err, "parsing integer"):
		return "integer values must be valid integers, no decimals allowed"
	case strings.Contains(err, "parsing boolean"):
		return "boolean values must be either 'true' or 'false'"
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
