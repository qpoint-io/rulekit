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
	case op_STARTS_WITH:
		return "starts_with"
	case op_ENDS_WITH:
		return "ends_with"
	case op_SUBDOMAIN_OF:
		return "subdomain_of"
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
	str := string(data)
	if str[0] == '\'' {
		// Convert single-quoted string to double-quoted
		str = str[1:len(str)-1]
		str = strings.ReplaceAll(str, `"`, "\\\"")
		str = strings.ReplaceAll(str, `\'`, `'`)
		str = `"` + str + `"`
	}
	return strconv.Unquote(str)
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
	raw := string(data)
	if strings.EqualFold(raw, "true") {
		return true, nil
	}
	if strings.EqualFold(raw, "false") {
		return false, nil
	}
	return false, fmt.Errorf("parsing boolean: unknown value %q", raw)
}

func parseRegex[T interface{ string | []byte }](data T) (*regexp.Regexp, error) {
	raw := string(data)
	pattern := raw[1:len(raw)-1]  // Remove the forward slashes
	return regexp.Compile(pattern)
}

func newValueToken(token_type int, data []byte) (valueToken, error) {
	v := valueToken{typ: token_type, raw: string(data)}
	if err := v.Parse(); err != nil {
		return valueToken{}, err
	}
	return v, nil
}

type valueToken struct {
	typ int
	raw string
	value any
}

func (v *valueToken) Parse() error {
	var (
		value any
		err error
	)
	switch v.typ {
	case token_STRING:
		value, err = parseString(v.raw)
		if err != nil {
			err = ValueParseError{v.typ, v.raw, err}
		}
	case token_INT:
		value, err = parseInt(v.raw)
		if err != nil {
			err = ValueParseError{v.typ, v.raw, err}
		}
	case token_FLOAT:
		value, err = parseFloat(v.raw)
		if err != nil {
			err = ValueParseError{v.typ, v.raw, err}
		}
	case token_BOOL:
		value, err = parseBool(v.raw)
		if err != nil {
			err = ValueParseError{v.typ, v.raw, err}
		}
	case token_IP:
		value = net.ParseIP(v.raw)
		if value == nil {
			err = ValueParseError{v.typ, v.raw, fmt.Errorf("invalid IP value %q", v.raw)}
		}
	case token_IP_CIDR:
		_, value, err = net.ParseCIDR(v.raw)
		if err != nil {
			err = ValueParseError{v.typ, v.raw, err}
		}
	case token_HEX_STRING:
		value, err = ParseHexString(v.raw)
		if err != nil {
			err = ValueParseError{v.typ, v.raw, err}
		}
	case token_REGEX:
		value, err = parseRegex(v.raw)
		if err != nil {
			err = ValueParseError{v.typ, v.raw, err}
		}
	case token_FIELD:
		// no-op
	default:
		err = fmt.Errorf("unsupported token type %s for value %q", valueTokenString(v.typ), v.raw)
	}
	if err != nil {
		return err
	}
	v.value = value
	return nil
}

func (v *valueToken) Valuer() Valuer {
	if v.typ == token_FIELD {
		return FieldValue(string(v.raw))
	}
	return LiteralValue[any]{
		raw: v.raw,
		value: v.value,
	}
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
	valueLiteral  []byte
	operator      int
	negate        bool
	arrayValue    []valueToken 
	valueToken    valueToken
}

// Type declarations for non-terminals (rules)
%type <rule> search_condition predicate
%type <operator> ineq_operator eq_operator
%type <negate> optional_negate
%type <arrayValue> array_values
// value tokens
%type <valueToken> value_token // all values
%type <valueToken> numeric_value_token // int or float values
%type <valueToken> array_value_token // array values
%type <valueToken> array_or_single_value_token // arrays or single values

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
%token op_STARTS_WITH op_ENDS_WITH op_SUBDOMAIN_OF
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
	numeric_value_token optional_negate ineq_operator numeric_value_token
	{
		$$ = withNegate($2, &nodeCompare{
			lv: $1.Valuer(),
			op: $3,
			rv: $4.Valuer(),
		})
	}
	// all values including numeric accept equality operators
	| array_or_single_value_token optional_negate eq_operator array_or_single_value_token
	{
		$$ = withNegate($2, &nodeCompare{
			lv: $1.Valuer(),
			op: $3,
			rv: $4.Valuer(),
		})
	}
	// op_MATCHES supports regex values
	| array_or_single_value_token optional_negate op_MATCHES token_REGEX
	{
		elem, err := newValueToken(token_REGEX, $4)
		if err != nil {
			rulelex.Error(err.Error())
			return 1
		}

		$$ = withNegate($2, &nodeMatch{
			lv: $1.Valuer(),
			rv: elem.Valuer(),
		})
	}
	| array_or_single_value_token
	{
		$$ = &nodeNotZero{$1.Valuer()}
	}
	// op_IN supports array values
	| array_or_single_value_token optional_negate op_IN array_value_token
	{
		values, ok := $4.value.([]any)
		if !ok {
			rulelex.Error(fmt.Errorf("parser error while handling array value %q", $4.raw).Error())
			return 1
		}

		$$ = withNegate($2, &nodeIn{
			lv: $1.Valuer(),
			rv: LiteralValue[[]any]{
				raw: $4.raw,
				value: values,
			},
		})
	}
	;

ineq_operator:
	op_GT        { $$ = op_GT }
	| op_GE      { $$ = op_GE }
	| op_LT      { $$ = op_LT }
	| op_LE      { $$ = op_LE }
	;

eq_operator:
	op_EQ              { $$ = op_EQ           }
	| op_NE            { $$ = op_NE           }
	| op_CONTAINS      { $$ = op_CONTAINS     }
	| op_STARTS_WITH   { $$ = op_STARTS_WITH  }
	| op_ENDS_WITH     { $$ = op_ENDS_WITH    }
	| op_SUBDOMAIN_OF  { $$ = op_SUBDOMAIN_OF }
	;

optional_negate:
	/* nothing */ { $$ = false }
	| op_NOT      { $$ = true  }
	;

// Array handling rules
array_values:
	value_token
	{
		$$ = []valueToken{$1}
	}
	| array_values token_COMMA value_token
	{
		$$ = append($1, $3)
	}
	;

array_value_token:
	token_LBRACKET array_values token_RBRACKET
	{
		raw_parts := make([]string, len($2))
		values := make([]any, len($2))
		for i, elem := range $2 {
			raw_parts[i] = elem.raw
			values[i] = elem.value
		}
		raw := fmt.Sprintf("[%s]", strings.Join(raw_parts, ", "))

		$$ = valueToken{
			typ: token_ARRAY,
			raw: raw,
			value: values,
		}
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
		v, err := newValueToken(token_STRING, $1)	
		if err != nil {
			rulelex.Error(err.Error())
			return 1
		}
		$$ = v
	}
	| token_BOOL
	{
		v, err := newValueToken(token_BOOL, $1)
		if err != nil {
			rulelex.Error(err.Error())
			return 1
		}
		$$ = v
	}
	| token_IP
	{
		v, err := newValueToken(token_IP, $1)
		if err != nil {
			rulelex.Error(err.Error())
			return 1
		}
		$$ = v
	}
	| token_IP_CIDR
	{
		v, err := newValueToken(token_IP_CIDR, $1)
		if err != nil {
			rulelex.Error(err.Error())
			return 1
		}
		$$ = v
	}
	| token_HEX_STRING
	{
		v, err := newValueToken(token_HEX_STRING, $1)
		if err != nil {
			rulelex.Error(err.Error())
			return 1
		}
		$$ = v
	}
	| token_REGEX
	{
		v, err := newValueToken(token_REGEX, $1)
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
		v, err := newValueToken(token_INT, $1)
		if err != nil {
			rulelex.Error(err.Error())
			return 1
		}
		$$ = v
	}
	| token_FLOAT
	{
		v, err := newValueToken(token_FLOAT, $1)
		if err != nil {
			rulelex.Error(err.Error())
			return 1
		}
		$$ = v
	}
	| token_FIELD
	{
		v, err := newValueToken(token_FIELD, $1)
		if err != nil {
			rulelex.Error(err.Error())
			return 1
		}
		$$ = v
	}
	;

%%
