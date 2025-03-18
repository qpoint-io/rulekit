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

%}

%union {
	rule      Rule
	data      []byte
	operator  int
	negate    bool
	values    []any
}

// Type declarations for non-terminals (rules)
%type <rule> search_condition predicate
%type <operator> comparison_operator ineq_operator eq_operator
%type <negate> optional_negate
%type <values> array_values array_value

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
		field, raw_value := string($1), string($4)
		negate := $2
		op := $3
		
		value, err := parseString(raw_value)
		if err != nil {
			rulelex.Error(fmt.Sprintf("parsing string: %v", err))
			return 1
		}
		
		$$ = withNegate(negate, &nodeCompare{
			predicate: predicate{field: field, raw_value: raw_value},
			op: op,
			value: value,
		})
	}
	| token_FIELD optional_negate eq_operator token_HEX_STRING
	{
		field, raw_value := string($1), string($4)
		negate := $2
		op := $3

		hs, err := ParseHexString(raw_value)
		if err != nil {
			rulelex.Error(fmt.Sprintf("parsing hex string: %v", err))
			return 1
		}
		$$ = withNegate(negate, &nodeCompare{
			predicate: predicate{field: field, raw_value: raw_value},
			op: op,
			value: hs,
		})
	}
	| token_FIELD optional_negate comparison_operator token_INT
	{
		field, raw_value := string($1), string($4)
		negate := $2
		op := $3
		
		value, err := parseInt(raw_value)
		if err != nil {
			rulelex.Error(err.Error())
			return 1
		}
		
		$$ = withNegate(negate, &nodeCompare{
			predicate: predicate{field: field, raw_value: raw_value},
			op: op,
			value: value,
		})
	}
	| token_FIELD optional_negate comparison_operator token_FLOAT
	{
		field, raw_value := string($1), string($4)
		negate := $2
		op := $3
		
		value, err := parseFloat(raw_value)
		if err != nil {
			rulelex.Error(fmt.Sprintf("parsing float: %v", err))
			return 1
		}
		
		$$ = withNegate(negate, &nodeCompare{
			predicate: predicate{field: field, raw_value: raw_value},
			op: op,
			value: value,
		})
	}
	| token_FIELD optional_negate eq_operator token_BOOL
	{
		field, raw_value := string($1), string($4)
		negate := $2
		op := $3
		
		value, err := parseBool(raw_value)
		if err != nil {
			rulelex.Error(err.Error())
			return 1
		}
		
		$$ = withNegate(negate, &nodeCompare{
			predicate: predicate{field: field, raw_value: raw_value},
			op: op,
			value: value,
		})
	}
	| token_FIELD optional_negate eq_operator token_IP
	{
		field, raw_value := string($1), string($4)
		negate := $2
		op := $3

		ip := net.ParseIP(raw_value)
		if ip == nil {
			rulelex.Error(fmt.Sprintf("parsing IP: invalid value %q", raw_value))
			return 1
		}
		
		$$ = withNegate(negate, &nodeCompare{
			predicate: predicate{field: field, raw_value: raw_value},
			op: op,
			value: ip,
		})
	}
	| token_FIELD optional_negate eq_operator token_IP_CIDR
	{
		field, raw_value := string($1), string($4)
		negate := $2
		op := $3

		_, ipnet, err := net.ParseCIDR(raw_value)
		if err != nil {
			rulelex.Error(fmt.Sprintf("parsing CIDR: invalid value %v", raw_value))
			return 1
		}
		
		$$ = withNegate(negate, &nodeCompare{
			predicate: predicate{field: field, raw_value: raw_value},
			op: op,
			value: ipnet,
		})
	}
	| token_FIELD optional_negate token_TEST_MATCHES token_REGEX
	{
		field, raw_value := string($1), string($4)
		pattern := raw_value[1:len(raw_value)-1]  // Remove the forward slashes
		negate := $2
		
		r_expr, err := regexp.Compile(pattern)
		if err != nil {
			rulelex.Error(fmt.Sprintf("parsing regular expression: %v", err))
			return 1
		}
		
		$$ = withNegate(negate, &nodeMatch{
			predicate: predicate{field: field, raw_value: raw_value},
			reg_expr: r_expr,
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
		values := $5
		
		raw_value := fmt.Sprintf("%v", values) // TODO
		
		$$ = withNegate(negate, &nodeCompare{
			predicate: predicate{field: field, raw_value: raw_value},
			op: op,
			value: values,
		})
	}
	| token_FIELD optional_negate token_TEST_IN token_LBRACKET array_values token_RBRACKET
	{
		field := string($1)
		negate := $2
		values := $5

		raw_value := fmt.Sprintf("%v", values) // TODO
		
		$$ = withNegate(negate, &nodeIn{
			predicate: predicate{field: field, raw_value: raw_value},
			values: values,
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

// Array handling rules with type-specific parsing
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
	token_STRING
	{
		value, err := parseString($1)
		if err != nil {
			rulelex.Error(fmt.Sprintf("parsing array string: %v", err))
			return 1
		}
		$$ = []any{value}
	}
	| token_INT
	{
		value, err := parseInt($1)
		if err != nil {
			rulelex.Error(err.Error())
			return 1
		}
		$$ = []any{value}
	}
	| token_FLOAT
	{
		value, err := parseFloat($1)
		if err != nil {
			rulelex.Error(fmt.Sprintf("parsing array float: %v", err))
			return 1
		}
		$$ = []any{value}
	}
	| token_BOOL
	{
		value, err := parseBool($1)
		if err != nil {
			rulelex.Error(err.Error())
			return 1
		}
		$$ = []any{value}
	}
	;

%%
