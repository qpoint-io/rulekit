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

func parseString[T interface{ string | []byte }](data T) (any, error) {
	str := string(data)
	if str[0] == '\'' {
		// Convert single-quoted string to double-quoted
		str = str[1:len(str)-1]
		str = strings.ReplaceAll(str, `"`, "\\\"")
		str = strings.ReplaceAll(str, `\'`, `'`)
		str = `"` + str + `"`
	}
	var err error
	str, err = strconv.Unquote(str)
	if err != nil {
		return nil, err
	}
	
	// Try to parse string value as a more specific type
	if ip := net.ParseIP(str); ip != nil {
		// Try IP address
		return ip, nil
	} else if _, ipnet, err := net.ParseCIDR(str); err == nil {
		// Try CIDR notation
		return ipnet, nil
	} else if strings.Count(str, ":") == 5 || strings.Count(str, ":") == 7 {
		// Try MAC address (if it looks like a MAC address format)
		if mac, err := net.ParseMAC(str); err == nil {
			return mac, nil
		}
	}
	return str, nil
}

func parseInt[T interface{ string | []byte }](data T) (any, error) {
	raw := string(data)
	if n, err := strconv.ParseInt(raw, 0, 64); err == nil {
		return n, nil
	}
	if n, err := strconv.ParseUint(raw, 0, 64); err == nil {
		return n, nil
	}
	return nil, fmt.Errorf("parsing integer: invalid value %q", raw)
}

func parseFloat[T interface{ string | []byte }](data T) (float64, error) {
	return strconv.ParseFloat(string(data), 64)
}

func parseBool[T interface{ string | []byte }](data T) (bool, error) {
	var val bool
	_, err := fmt.Sscanf(string(data), "%t", &val)
	return val, err
}

func parseRegex[T interface{ string | []byte }](data T) (*regexp.Regexp, error) {
	raw := string(data)
	pattern := raw[1:len(raw)-1]  // Remove the forward slashes
	return regexp.Compile(pattern)
}

func parseValueToken(typ int, rawBytes []byte) (Rule, error) {
	raw := string(rawBytes)
	var (
		value any
		err error
	)
	switch typ {
	case token_STRING:
		value, err = parseString(raw)
	case token_INT:
		value, err = parseInt(raw)
	case token_FLOAT:
		value, err = parseFloat(raw)
	case token_BOOL:
		value, err = parseBool(raw)
	case token_IP:
		value = net.ParseIP(raw)
	case token_IP_CIDR:
		_, value, err = net.ParseCIDR(raw)
	case token_HEX_STRING:
		value, err = ParseHexString(raw)
	case token_REGEX:
		value, err = parseRegex(raw)
	default:
		err = fmt.Errorf("unknown parseValueToken type")
	}
	if err != nil {
		return nil, ValueParseError{typ, string(raw), err}
	}
	return &LiteralValue[any]{
		raw: string(raw),
		value: value,
	}, nil
}

type valueToken struct {
	typ int
	raw string
	value any
}

func valueTokenString(typ int) string {
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
	case token_FIELD:
		return "field identifier"
	case token_LPAREN:
		return `"("`
	case token_RPAREN:
		return `")"`
	case token_LBRACKET:
		return `"["`
	case token_RBRACKET:
		return `"]"`
	case token_COMMA:
		return `","`
	default:
		return "unknown"
	}
}

type ValueParseError struct {
	TokenType int
	Value     string
	Err       error
}

func (e ValueParseError) Error() string {
	return fmt.Sprintf("parsing %s value %q: %v", valueTokenString(e.TokenType), e.Value, e.Err)
}

%}

%union {
	rule          Rule
	operator      int
	valueLiteral  []byte
	arrayValue    []Rule
}

// Type declarations for non-terminals (rules)
%type <rule> search_condition predicate
%type <operator> ineq_operator eq_operator
%type <arrayValue> array_values
// value tokens
%type <rule> value_token // all values
%type <rule> numeric_value_token // int or float values
%type <rule> array_value_token // array values
%type <rule> array_or_single_value_token // arrays or single values

%token <valueLiteral> token_FIELD
%token <valueLiteral> token_STRING token_HEX_STRING
%token <valueLiteral> token_INT token_FLOAT
%token <valueLiteral> token_BOOL
%token <valueLiteral> token_IP_CIDR
%token <valueLiteral> token_IP
%token <valueLiteral> token_REGEX

// Tokens without values
%token op_NOT op_AND op_OR
%token token_LPAREN token_RPAREN
%token token_LBRACKET token_RBRACKET
%token token_COMMA
%token op_EQ op_NE
%token op_GT op_GE op_LT op_LE
%token op_CONTAINS op_MATCHES op_IN
%token token_ARRAY
%token token_ERROR

// Operator precedence
%left op_AND
%left op_OR
%right op_NOT

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
	// numeric values accept additional inequality operators
	numeric_value_token ineq_operator numeric_value_token
	{
		$$ = &nodeCompare{
			lv: $1,
			op: $2,
			rv: $3,
		}
	}
	// all values including numeric accept equality operators
	| array_or_single_value_token eq_operator array_or_single_value_token
	{
		$$ = &nodeCompare{
			lv: $1,
			op: $2,
			rv: $3,
		}
	}
	// op_MATCHES supports regex values
	| array_or_single_value_token op_MATCHES token_REGEX
	{
		elem, err := parseValueToken(token_REGEX, $3)
		if err != nil {
			rulelex.Error(err.Error())
			return 1
		}

		$$ = &nodeMatch{
			lv: $1,
			rv: elem,
		}
	}
	| array_or_single_value_token
	{
		$$ = $1
	}
	// op_IN supports array values
	| array_or_single_value_token op_IN array_value_token
	{
		$$ = &nodeIn{
			lv: $1,
			rv: $3,
		}
	}
	// op_IN supports IP CIDR values
	| array_or_single_value_token op_IN token_IP_CIDR
	{
		v, err := parseValueToken(token_IP_CIDR, $3)
		if err != nil {
			rulelex.Error(err.Error())
			return 1
		}

		$$ = &nodeCompare{
			lv: $1,
			op: op_EQ,
			rv: v,
		}
	}
	;

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

// Array handling rules
array_values:
	value_token
	{
		$$ = []Rule{$1}
	}
	| array_values token_COMMA value_token
	{
		$$ = append($1, $3)
	}
	;

array_value_token:
	token_LBRACKET array_values token_RBRACKET
	{
		$$ = newArrayValue($2)
	}
	;

array_or_single_value_token:
	value_token         { $$ = $1 }
	| array_value_token { $$ = $1 }
	;

// value tokens
value_token:
	numeric_value_token { $$ = $1 }
	| token_STRING
	{
		v, err := parseValueToken(token_STRING, $1)	
		if err != nil {
			rulelex.Error(err.Error())
			return 1
		}
		$$ = v
	}
	| token_BOOL
	{
		v, err := parseValueToken(token_BOOL, $1)
		if err != nil {
			rulelex.Error(err.Error())
			return 1
		}
		$$ = v
	}
	| token_IP
	{
		v, err := parseValueToken(token_IP, $1)
		if err != nil {
			rulelex.Error(err.Error())
			return 1
		}
		$$ = v
	}
	| token_IP_CIDR
	{
		v, err := parseValueToken(token_IP_CIDR, $1)
		if err != nil {
			rulelex.Error(err.Error())
			return 1
		}
		$$ = v
	}
	| token_HEX_STRING
	{
		v, err := parseValueToken(token_HEX_STRING, $1)
		if err != nil {
			rulelex.Error(err.Error())
			return 1
		}
		$$ = v
	}
	| token_REGEX
	{
		v, err := parseValueToken(token_REGEX, $1)
		if err != nil {
			rulelex.Error(err.Error())
			return 1
		}
		$$ = v
	}
	;

numeric_value_token:
	token_INT
	{
		v, err := parseValueToken(token_INT, $1)
		if err != nil {
			rulelex.Error(err.Error())
			return 1
		}
		$$ = v
	}
	| token_FLOAT
	{
		v, err := parseValueToken(token_FLOAT, $1)
		if err != nil {
			rulelex.Error(err.Error())
			return 1
		}
		$$ = v
	}
	| token_FIELD
	{
		$$ = FieldValue(string($1))
	}
	;

%%
