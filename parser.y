%{
package rulekit

import (
	"fmt"
	"io"
	"net"
	"os"
	"regexp"
	"strconv"
	"strings"
)

var ruleDebugWriter io.Writer = os.Stderr

func init() {
	SetErrorVerbose(true) // default to true
}

// SetDebugLevel sets the debug verbosity level
func SetDebugLevel(level int) {
	ruleDebug = level
}

func SetDebugWriter(w io.Writer) {
	ruleDebugWriter = w
}

// SetErrorVerbose enables or disables verbose error reporting
func SetErrorVerbose(verbose bool) {
	ruleErrorVerbose = verbose
}

func operatorToString(op int) string {
	switch op {
	case op_EQ:
		return "=="
	case op_NE:
		return "!="
	case op_GT:
		return ">"
	case op_GE:
		return ">="
	case op_LT:
		return "<"
	case op_LE:
		return "<="
	case op_CONTAINS:
		return "contains"
	case op_MATCHES:
		return "matches"
	case op_IN:
		return "in"
	default:
		return "unknown"
	}
}

func withNegate(negate bool, node Rule) Rule {
	if negate {
		return &nodeNot{right: node}
	}
	return node
}

// Add these type-specific parsing functions in the Go code section
func parseString[T interface{ string | []byte }](data T) (string, error) {
	raw_value := string(data)
	if raw_value[0] == '\'' {
		// Convert single-quoted string to double-quoted
		inner := raw_value[1:len(raw_value)-1]
		escaped := strings.ReplaceAll(inner, `"`, "\\\"")
		escaped = strings.ReplaceAll(escaped, `\'`, `'`)
		raw_value = `"` + escaped + `"`
	}
	return strconv.Unquote(raw_value)
}

func parseInt[T interface{ string | []byte }](data T) (any, error) {
	raw_value := string(data)
	if n, err := strconv.ParseInt(raw_value, 0, 64); err == nil {
		return n, nil
	}
	if n, err := strconv.ParseUint(raw_value, 0, 64); err == nil {
		return n, nil
	}
	return nil, fmt.Errorf("parsing integer: invalid value %q", raw_value)
}

func parseFloat[T interface{ string | []byte }](data T) (float64, error) {
	return strconv.ParseFloat(string(data), 64)
}

func parseBool[T interface{ string | []byte }](data T) (bool, error) {
	raw_value := string(data)
	if strings.EqualFold(raw_value, "true") {
		return true, nil
	}
	if strings.EqualFold(raw_value, "false") {
		return false, nil
	}
	return false, fmt.Errorf("parsing boolean: unknown value %q", raw_value)
}

func parseRegex[T interface{ string | []byte }](data T) (*regexp.Regexp, error) {
	raw_value := string(data)
	pattern := raw_value[1:len(raw_value)-1]  // Remove the forward slashes
	return regexp.Compile(pattern)
}

// Add a new type to represent parsed values
type parsedValue struct {
	raw_value string
	value     any
}

// Helper to format array raw value nicely
func formatArrayRawValue(elements []parsedValue) string {
	parts := make([]string, len(elements))
	for i, elem := range elements {
		parts[i] = elem.raw_value
	}
	return fmt.Sprintf("[%s]", strings.Join(parts, ", "))
}

func extractValues(elements []parsedValue) []any {
	values := make([]any, len(elements))
	for i, elem := range elements {
		values[i] = elem.value
	}
	return values
}

func parseToken(token_type int, data []byte) (parsedValue, error) {
	var (
		raw = string(data)
		value any
		err error
	)
	switch token_type {
	case token_STRING:
		value, err = parseString(data)
	case token_INT:
		value, err = parseInt(data)
	case token_FLOAT:
		value, err = parseFloat(data)
	case token_BOOL:
		value, err = parseBool(data)
	case token_IP:
		if value = net.ParseIP(raw); value == nil {
			err = fmt.Errorf("invalid IP value %q", raw)
		}
	case token_IP_CIDR:
		_, value, err = net.ParseCIDR(raw)
	case token_HEX_STRING:
		value, err = ParseHexString(raw)
	case token_REGEX:
		value, err = parseRegex(data)
	default:
		err = fmt.Errorf("unsupported token type %d", token_type)
	}
	if err != nil {
		return parsedValue{}, fmt.Errorf("parsing %s: %v", tokenTypeString(token_type), err)
	}
	return parsedValue{raw_value: raw, value: value}, nil
}

// Add token type struct for mid-rule actions
type tokenType struct {
	typ int
	data []byte
}

func tokenTypeString(typ int) string {
	switch typ {
	case token_STRING:
		return "string"
	case token_INT:
		return "integer"
	case token_FLOAT:
		return "float"
	case token_BOOL:
		return "boolean"
	case token_IP:
		return "IP"
	case token_IP_CIDR:
		return "CIDR"
	case token_HEX_STRING:
		return "hex string"
	case token_REGEX:
		return "regex"
	default:
		return "unknown"
	}
}

// Add these helper functions to the Go section
func makeCompareNode(field string, negate bool, op int, elem parsedValue) Rule {
	return withNegate(negate, &nodeCompare{
		predicate: predicate{
			field: field, 
			raw_value: elem.raw_value,
		},
		op: op,
		value: elem.value,
	})
}

func makeArrayCompareNode(field string, negate bool, op int, elems []parsedValue) Rule {
	return withNegate(negate, &nodeCompare{
		predicate: predicate{
			field: field,
			raw_value: formatArrayRawValue(elems),
		},
		op: op,
		value: extractValues(elems),
	})
}
%}

%union {
	rule      Rule
	data      []byte
	operator  int
	negate    bool
	arrayElems   []parsedValue  // Change type from []any to []parsedValue
	tokType     tokenType    // Add new type for mid-rule actions
}

// Type declarations for non-terminals (rules)
%type <rule> search_condition predicate
%type <operator> comparison_operator ineq_operator eq_operator
%type <negate> optional_negate
%type <arrayElems> array_values array_value
%type <tokType> value_token

%token <data> token_FIELD
%token <data> token_STRING token_HEX_STRING
%token <data> token_INT token_FLOAT
%token <data> token_BOOL
%token <data> token_IP_CIDR
%token <data> token_IP
%token <data> token_REGEX

// Tokens without values
%token op_NOT op_AND op_OR
%token token_LPAREN token_RPAREN
%token token_LBRACKET token_RBRACKET
%token token_COMMA
%token op_EQ op_NE
%token op_GT op_GE op_LT op_LE
%token op_CONTAINS op_MATCHES op_IN
%token token_ERROR

// Operator precedence
%left op_AND
%left op_OR
%left op_NOT

%%
search_condition:
	predicate
	{
		$$ = $1
		rulelex.Result($$)
	}
	| search_condition op_AND search_condition
	{
		$$ = &nodeAnd{left: $1, right: $3}
		rulelex.Result($$)
	}
	| search_condition op_OR search_condition
	{
		$$ = &nodeOr{left: $1, right: $3}
		rulelex.Result($$)
	}
	| op_NOT search_condition
	{
		$$ = &nodeNot{right: $2}
		rulelex.Result($$)
	}
	| token_LPAREN search_condition token_RPAREN
	{
		$$ = $2
		rulelex.Result($$)
	}
	;

predicate:
	token_FIELD optional_negate eq_operator token_STRING
	{
		elem, err := parseToken(token_STRING, $4)
		if err != nil {
			rulelex.Error(err.Error())
			return 1
		}
		$$ = makeCompareNode(string($1), $2, $3, elem)
	}
	| token_FIELD optional_negate eq_operator token_HEX_STRING
	{
		elem, err := parseToken(token_HEX_STRING, $4)
		if err != nil {
			rulelex.Error(err.Error())
			return 1
		}
		$$ = makeCompareNode(string($1), $2, $3, elem)
	}
	| token_FIELD optional_negate comparison_operator token_INT
	{
		elem, err := parseToken(token_INT, $4)
		if err != nil {
			rulelex.Error(err.Error())
			return 1
		}
		
		$$ = makeCompareNode(string($1), $2, $3, elem)
	}
	| token_FIELD optional_negate comparison_operator token_FLOAT
	{
		elem, err := parseToken(token_FLOAT, $4)
		if err != nil {
			rulelex.Error(err.Error())
			return 1
		}

		$$ = makeCompareNode(string($1), $2, $3, elem)
	}
	| token_FIELD optional_negate eq_operator token_BOOL
	{
		elem, err := parseToken(token_BOOL, $4)
		if err != nil {
			rulelex.Error(err.Error())
			return 1
		}
		
		$$ = makeCompareNode(string($1), $2, $3, elem)
	}
	| token_FIELD optional_negate eq_operator token_IP
	{
		elem, err := parseToken(token_IP, $4)
		if err != nil {
			rulelex.Error(err.Error())
			return 1
		}
		
		$$ = makeCompareNode(string($1), $2, $3, elem)
	}
	| token_FIELD optional_negate eq_operator token_IP_CIDR
	{
		elem, err := parseToken(token_IP_CIDR, $4)
		if err != nil {
			rulelex.Error(err.Error())
			return 1
		}
		
		$$ = makeCompareNode(string($1), $2, $3, elem)
	}
	| token_FIELD optional_negate op_MATCHES token_REGEX
	{
		elem, err := parseToken(token_REGEX, $4)
		if err != nil {
			rulelex.Error(err.Error())
			return 1
		}

		rgxp, ok := elem.value.(*regexp.Regexp)
		if !ok {
			// code error; parseToken should always return a *regexp.Regexp for a token_REGEX
			rulelex.Error(fmt.Errorf("parser error while handling regex value %q", elem.raw_value).Error())
			return 1
		}

		$$ = withNegate($2, &nodeMatch{
			predicate: predicate{field: string($1), raw_value: elem.raw_value},
			reg_expr: rgxp,
		})
	}
	| token_FIELD
	{
		$$ = &nodeNotZero{string($1)}
	}
	| token_FIELD optional_negate eq_operator token_LBRACKET array_values token_RBRACKET
	{
		$$ = makeArrayCompareNode(string($1), $2, $3, $5)
	}
	| token_FIELD optional_negate op_IN token_LBRACKET array_values token_RBRACKET
	{
		$$ = withNegate($2, &nodeIn{
			predicate: predicate{
				field: string($1),
				raw_value: formatArrayRawValue($5),
			},
			values: extractValues($5),
		})
	}
	;

comparison_operator: ineq_operator | eq_operator;

ineq_operator:
	op_GT        { $$ = op_GT }
	| op_GE      { $$ = op_GE }
	| op_LT      { $$ = op_LT }
	| op_LE      { $$ = op_LE }
	;

eq_operator:
	op_EQ         { $$ = op_EQ       }
	| op_NE       { $$ = op_NE       }
	| op_CONTAINS { $$ = op_CONTAINS }
	;

optional_negate:
	{ $$ = false }
	| op_NOT { $$ = true };

// Array handling rules
array_values:
	array_value
	{
		$$ = $1
	}
	| array_values token_COMMA array_value
	{
		$$ = append($1, $3...)
	}
	;

array_value:
	value_token
	{
		elem, err := parseToken($1.typ, $1.data)
		if err != nil {
			rulelex.Error(err.Error())
			return 1
		}
		$$ = []parsedValue{elem}
	}
	;

value_token:
	token_STRING
	{
		$$ = tokenType{typ: token_STRING, data: $1}
	}
	| token_INT
	{
		$$ = tokenType{typ: token_INT, data: $1}
	}
	| token_FLOAT
	{
		$$ = tokenType{typ: token_FLOAT, data: $1}
	}
	| token_BOOL
	{
		$$ = tokenType{typ: token_BOOL, data: $1}
	}
	| token_IP
	{
		$$ = tokenType{typ: token_IP, data: $1}
	}
	| token_IP_CIDR
	{
		$$ = tokenType{typ: token_IP_CIDR, data: $1}
	}
	| token_HEX_STRING
	{
		$$ = tokenType{typ: token_HEX_STRING, data: $1}
	}
	;

%%
