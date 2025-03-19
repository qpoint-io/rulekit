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
	case token_TEST_EQ:
		return "=="
	case token_TEST_NE:
		return "!="
	case token_TEST_GT:
		return ">"
	case token_TEST_GE:
		return ">="
	case token_TEST_LT:
		return "<"
	case token_TEST_LE:
		return "<="
	case token_TEST_CONTAINS:
		return "contains"
	case token_TEST_MATCHES:
		return "matches"
	case token_TEST_IN:
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
%type <tokType> array_token

%token <data> token_FIELD
%token <data> token_STRING token_HEX_STRING
%token <data> token_INT token_FLOAT
%token <data> token_BOOL
%token <data> token_IP_CIDR
%token <data> token_IP
%token <data> token_REGEX

// Tokens without values
%token token_TEST_NOT token_TEST_AND token_TEST_OR
%token token_LPAREN token_RPAREN
%token token_LBRACKET token_RBRACKET
%token token_COMMA
%token token_TEST_EQ token_TEST_NE
%token token_TEST_GT token_TEST_GE token_TEST_LT token_TEST_LE
%token token_TEST_CONTAINS token_TEST_MATCHES token_TEST_IN
%token token_ERROR

// Operator precedence
%left token_TEST_AND
%left token_TEST_OR
%left token_TEST_NOT

%%
search_condition:
	predicate
	{
		$$ = $1
		rulelex.Result($$)
	}
	| search_condition token_TEST_AND search_condition
	{
		$$ = &nodeAnd{left: $1, right: $3}
		rulelex.Result($$)
	}
	| search_condition token_TEST_OR search_condition
	{
		$$ = &nodeOr{left: $1, right: $3}
		rulelex.Result($$)
	}
	| token_TEST_NOT search_condition
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
		field := string($1)
		negate := $2
		op := $3
		
		elem, err := parseToken(token_STRING, $4)
		if err != nil {
			rulelex.Error(err.Error())
			return 1
		}
		
		$$ = withNegate(negate, &nodeCompare{
			predicate: predicate{field: field, raw_value: elem.raw_value},
			op: op,
			value: elem.value,
		})
	}
	| token_FIELD optional_negate eq_operator token_HEX_STRING
	{
		field := string($1)
		negate := $2
		op := $3

		elem, err := parseToken(token_HEX_STRING, $4)
		if err != nil {
			rulelex.Error(err.Error())
			return 1
		}
		$$ = withNegate(negate, &nodeCompare{
			predicate: predicate{field: field, raw_value: elem.raw_value},
			op: op,
			value: elem.value,
		})
	}
	| token_FIELD optional_negate comparison_operator token_INT
	{
		field := string($1)
		negate := $2
		op := $3

		elem, err := parseToken(token_INT, $4)
		if err != nil {
			rulelex.Error(err.Error())
			return 1
		}
		
		$$ = withNegate(negate, &nodeCompare{
			predicate: predicate{field: field, raw_value: elem.raw_value},
			op: op,
			value: elem.value,
		})
	}
	| token_FIELD optional_negate comparison_operator token_FLOAT
	{
		field := string($1)
		negate := $2
		op := $3

		elem, err := parseToken(token_FLOAT, $4)
		if err != nil {
			rulelex.Error(err.Error())
			return 1
		}
		
		$$ = withNegate(negate, &nodeCompare{
			predicate: predicate{field: field, raw_value: elem.raw_value},
			op: op,
			value: elem.value,
		})
	}
	| token_FIELD optional_negate eq_operator token_BOOL
	{
		field := string($1)
		negate := $2
		op := $3

		elem, err := parseToken(token_BOOL, $4)
		if err != nil {
			rulelex.Error(err.Error())
			return 1
		}
		
		$$ = withNegate(negate, &nodeCompare{
			predicate: predicate{field: field, raw_value: elem.raw_value},
			op: op,
			value: elem.value,
		})
	}
	| token_FIELD optional_negate eq_operator token_IP
	{
		field := string($1)
		negate := $2
		op := $3

		elem, err := parseToken(token_IP, $4)
		if err != nil {
			rulelex.Error(err.Error())
			return 1
		}
		
		$$ = withNegate(negate, &nodeCompare{
			predicate: predicate{field: field, raw_value: elem.raw_value},
			op: op,
			value: elem.value,
		})
	}
	| token_FIELD optional_negate eq_operator token_IP_CIDR
	{
		field := string($1)
		negate := $2
		op := $3

		elem, err := parseToken(token_IP_CIDR, $4)
		if err != nil {
			rulelex.Error(err.Error())
			return 1
		}
		
		$$ = withNegate(negate, &nodeCompare{
			predicate: predicate{field: field, raw_value: elem.raw_value},
			op: op,
			value: elem.value,
		})
	}
	| token_FIELD optional_negate token_TEST_MATCHES token_REGEX
	{
		field := string($1)
		negate := $2
		
		elem, err := parseToken(token_REGEX, $4)
		if err != nil {
			rulelex.Error(err.Error())
			return 1
		}
		

		$$ = withNegate(negate, &nodeMatch{
			predicate: predicate{field: field, raw_value: elem.raw_value},
			reg_expr: elem.value.(*regexp.Regexp),
		})
	}
	| token_FIELD
	{
		$$ = &nodeNotZero{string($1)}
	}
	| token_FIELD optional_negate eq_operator token_LBRACKET array_values token_RBRACKET
	{
		field := string($1)
		negate := $2
		op := $3
		elements := $5
		
		$$ = withNegate(negate, &nodeCompare{
			predicate: predicate{
				field: field,
				raw_value: formatArrayRawValue(elements),
			},
			op: op,
			value: extractValues(elements),
		})
	}
	| token_FIELD optional_negate token_TEST_IN token_LBRACKET array_values token_RBRACKET
	{
		field := string($1)
		negate := $2
		elements := $5
		
		$$ = withNegate(negate, &nodeIn{
			predicate: predicate{
				field: field,
				raw_value: formatArrayRawValue(elements),
			},
			values: extractValues(elements),
		})
	}
	;

comparison_operator: ineq_operator | eq_operator;

ineq_operator:
	token_TEST_GT        { $$ = token_TEST_GT }
	| token_TEST_GE      { $$ = token_TEST_GE }
	| token_TEST_LT      { $$ = token_TEST_LT }
	| token_TEST_LE      { $$ = token_TEST_LE }
	;

eq_operator:
	token_TEST_EQ         { $$ = token_TEST_EQ       }
	| token_TEST_NE       { $$ = token_TEST_NE       }
	| token_TEST_CONTAINS { $$ = token_TEST_CONTAINS }
	;

optional_negate:
	{ $$ = false }
	| token_TEST_NOT { $$ = true };

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
	array_token
	{
		elem, err := parseToken($1.typ, $1.data)
		if err != nil {
			rulelex.Error(err.Error())
			return 1
		}
		$$ = []parsedValue{elem}
	}
	;

array_token:
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
