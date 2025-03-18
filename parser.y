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

%}

%union {
	rule      Rule
	data      []byte
	operator  int
	negate    bool
}

// Type declarations for non-terminals (rules)
%type <rule> search_condition predicate
%type <operator> comparison_operator ineq_operator eq_operator
%type <negate> optional_negate

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
%token token_TEST_EQ token_TEST_NE
%token token_TEST_GT token_TEST_GE token_TEST_LT token_TEST_LE
%token token_TEST_CONTAINS token_TEST_MATCHES
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

		value := raw_value
		if value[0] == '\'' {
            // Convert single-quoted string to double-quoted
            inner := value[1:len(value)-1]                // Remove outer quotes
            escaped := strings.ReplaceAll(inner, `"`, "\\\"")  // Escape any double quotes
            escaped = strings.ReplaceAll(escaped, `\'`, `'`)  // Unescape single quotes
            value = `"` + escaped + `"`                      // Wrap in double quotes
        }
		unquoted, err := strconv.Unquote(value)
		if err != nil {
			rulelex.Error(fmt.Sprintf("parsing string: %v", err))
			return 1
		}
		
		$$ = withNegate(negate, &nodeCompare{
			predicate: predicate{field: field, raw_value: raw_value},
			op: op,
			value: unquoted,
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

		var num any
		// attempt to parse as int64
		if n, err := strconv.ParseInt(raw_value, 0, 64); err == nil {
			num = n
		} else if n, err := strconv.ParseUint(raw_value, 0, 64); err == nil {
			// if value is out of range for int64, attempt to parse as uint64
			num = n
		} else {
			rulelex.Error(fmt.Sprintf("parsing integer: %v", err))
			return 1
		}
		
		$$ = withNegate(negate, &nodeCompare{
			predicate: predicate{field: field, raw_value: raw_value},
			op: op,
			value: num,
		})
	}
	| token_FIELD optional_negate comparison_operator token_FLOAT
	{
		field, raw_value := string($1), string($4)
		negate := $2
		op := $3

		num, err := strconv.ParseFloat(raw_value, 64)
		if err != nil {
			rulelex.Error(fmt.Sprintf("parsing %q: %v", raw_value, err))
			return 1
		}
		
		$$ = withNegate(negate, &nodeCompare{
			predicate: predicate{field: field, raw_value: raw_value},
			op: op,
			value: num,
		})
	}
	| token_FIELD optional_negate eq_operator token_BOOL
	{
		field, raw_value := string($1), string($4)
		negate := $2
		op := $3

		var value bool
		if strings.EqualFold(raw_value, "true") {
			value = true
		} else if strings.EqualFold(raw_value, "false") {
			value = false
		} else {
			rulelex.Error(fmt.Sprintf("parsing boolean: unknown value %q", raw_value))
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

%%
