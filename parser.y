%{
package rulekit
%}

%union {
	rule          Rule
	operator      int
	valueLiteral  []byte
	arrayValue    []Rule
}

// Type declarations for non-terminals (rules)
%type <rule> search_condition predicate
%type <rule> function_call
%type <operator> ineq_operator eq_operator
%type <arrayValue> array_values
// value tokens
%type <rule> value_token // all values
%type <rule> numeric_value_token // int or float values
%type <rule> array_value_token // array values
%type <rule> array_or_single_value_token // arrays or single values
%type <arrayValue> function_arguments // function arguments

%token <valueLiteral> token_FIELD
%token <valueLiteral> token_FUNCTION
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
	function_call         { $$ = $1 }
	| value_token         { $$ = $1 }
	| array_value_token   { $$ = $1 }
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
	| token_FUNCTION
	{
		// there is no syntatic difference between a function call and a field name
		// so an isolated function name is treated as a field name
		$$ = FieldValue(string($1))
	}
	;

function_call:
	token_FUNCTION token_LPAREN function_arguments token_RPAREN
	{
		fv := newFunctionValue(string($1), $3)
		if err := fv.ValidateStdlibFnArgs(); err != nil {
			// if this is a stdlib function, validate arguments early at parse time
			// rather than eval
			rulelex.Error(err.Error())
			return 1
		}
		$$ = fv
	}
	;

function_arguments:
	array_or_single_value_token
	{
		$$ = []Rule{$1}
	}
	| function_arguments token_COMMA array_or_single_value_token
	{
		$$ = append($1, $3)
	}
	| /* nothing */
	{
		$$ = ([]Rule)(nil)
	}
	;

%%
