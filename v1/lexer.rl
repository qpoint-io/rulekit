package rulekit

import (
	"fmt"
)

%%{
	machine ruleLexerImpl;

	# Basic types
	# ---

	int    = ('-' | '+')? digit+;
	float  = ('-' | '+')? digit* '.' digit+;
	bool   = 'true'i | 'false'i;
	
	# String types
	# ---

	dstring = '"' ([^"] | '\\n' | '\\t' | '\\r' | '\\"')* '"';
	sstring = "'" ([^'] | '\\n' | '\\t' | '\\r' | '\\\'')* "'";
	string  = dstring | sstring;
	# hex values e.g. 47:45:54 == "GET"
	hex   = [0-9a-fA-F];
	hex_string = hex{2} (':' hex{2})*;
	
	# Network types
	# ---
	
	octet = digit | ( 0x31..0x39 digit ) | ( "1" digit{2} ) |( "2" 0x30..0x34 digit ) | ( "25" 0x30..0x35 );
	ipv4  = octet '.' octet '.' octet '.' octet;
	h16   = hex{1,4};
	ls32  = ( h16 ":" h16 ) | ipv4;
	ipv6  = ( ( h16 ":" ){6} ls32 ) |
	       ( "::" ( h16 ":" ){5} ls32 ) |
	       ( h16? "::" ( h16 ":" ){4} ls32 ) |
	       ( ( ( h16 ":" )? h16 )? "::" ( h16 ":" ){3} ls32 ) |
	       ( ( ( h16 ":" ){,2} h16 )? "::" ( h16 ":" ){2} ls32 ) |
	       ( ( ( h16 ":" ){,3} h16 )? "::" h16 ":" ls32 ) |
	       ( ( ( h16 ":" ){,4} h16 )? "::" ls32 ) |
	       ( ( ( h16 ":" ){,5} h16 )? "::" h16 ) |
	       ( ( ( h16 ":" ){,6} h16 )? "::" );
	ip = ipv4 | ipv6;
	ip_cidr = ip '/' digit{1,2};
	
	# MAC addresses e.g. 47:45:54 or 47-45-54
	# mac_delim = ':' | '-';
	# mac = hex{2} (mac_delim hex{2}){5,6};

	# Regex types
	# ---
	
	escaped_regex_char = '\\' any;
	# /some\/thing/ -style regex literal
    not_slash_or_escape = any - ('/' | '\\');
	regex_forward_slash = '/' ( not_slash_or_escape | escaped_regex_char )* '/';
	# |some/thing|  -style regex literal
    not_pipe_or_escape = any - ('|' | '\\');
	regex_pipe = '|' ( not_pipe_or_escape | escaped_regex_char )* '|';

    regex_pattern = regex_forward_slash | regex_pipe;

	# Whitespace and comments
	# ---
	
	ws = [ \t\n\r];
	comment_line  = '--' [^\n]* '\n'?;
    comment_block = '/*' (any - '*/')* '*/';

	field_char = alpha | digit | '_' | '.' | '-';
    field = (alpha | '_') field_char*;  # Must start with alpha or underscore
	
	# Function names (similar to fields but can't contain dots)
	function_char = alpha | digit | '_';
	function = (alpha | '_') function_char*;  # Must start with alpha or underscore
	
	# --- lexer logic ---
	
	main := |*
        # Skip comments and whitespace
        comment_line | comment_block | ws => { /* skip */ };

		# Control
		'(' => { token_kind = token_LPAREN;   fbreak; };
		')' => { token_kind = token_RPAREN;   fbreak; };
		'[' => { token_kind = token_LBRACKET; fbreak; };
		']' => { token_kind = token_RBRACKET; fbreak; };
		',' => { token_kind = token_COMMA;    fbreak; };

		# Logical operators
		('!' | 'not'i)  => { token_kind = op_NOT; fbreak; };
		('&&' | 'and'i) => { token_kind = op_AND; fbreak; };
		('||' | 'or'i)  => { token_kind = op_OR;  fbreak; };

		# Comparison operators
		('==' | 'eq'i) => { token_kind = op_EQ; fbreak; };
		('!=' | 'ne'i) => { token_kind = op_NE; fbreak; };
		('<' | 'lt'i)  => { token_kind = op_LT; fbreak; };
		('<=' | 'le'i) => { token_kind = op_LE; fbreak; };
		('>' | 'gt'i)  => { token_kind = op_GT; fbreak; };
		('>=' | 'ge'i) => { token_kind = op_GE; fbreak; };

		'contains'i         => { token_kind = op_CONTAINS; fbreak; };
		('=~' | 'matches'i) => { token_kind = op_MATCHES;  fbreak; };
		'in'i               => { token_kind = op_IN;       fbreak; };

		# Values
		int    => { token_kind = token_INT;    fbreak; };
		float  => { token_kind = token_FLOAT;  fbreak; };
		bool   => { token_kind = token_BOOL;   fbreak; };
		string => { token_kind = token_STRING; fbreak; };

		ip            => { token_kind = token_IP;         fbreak; };
		ip_cidr       => { token_kind = token_IP_CIDR;    fbreak; };
		hex_string    => { token_kind = token_HEX_STRING; fbreak; };
		regex_pattern => { token_kind = token_REGEX;      fbreak; };

		function => { token_kind = token_FUNCTION; fbreak; };

		# Field names (allow alphanumeric and dots with restrictions)
		field => { token_kind = token_FIELD; fbreak; };

        # Add an error rule at the end to catch any unrecognized characters
        any => {
            lexer.Error(fmt.Sprintf("unexpected character: %q", safeIndex(lexer.data, lexer.ts, lexer.te)))
            return token_ERROR
        };
	*|;
	
	write data;
	variable data lexer.data;
	variable cs lexer.cs;
	variable p lexer.p;
	variable pe lexer.pe;
	variable act lexer.act;
	variable ts lexer.ts;
	variable te lexer.te;
	variable eof lexer.eof;

}%%

type ruleLexerImpl struct {
	data []byte
	cs   int
	p    int
	pe   int
	act  int
	ts   int
	te   int
	eof  int
	result Rule
	err   string
}

func newLex(line []byte) *ruleLexerImpl {
	lexer := ruleLexerImpl{data: line}
	%%write init;
	lexer.pe = len(line)
	lexer.eof = len(line)
	return &lexer
}

func (lexer *ruleLexerImpl) Lex(lval *ruleSymType) int {
    token_kind := 0
	%% write exec;
    if lexer.cs != ruleLexerImpl_error {
		lval.valueLiteral = safeIndex(lexer.data, lexer.ts, lexer.te)
    }
	if ruleDebug > 4 {
		fmt.Printf("Token text: %s\n", string(lval.valueLiteral))
	}

	return token_kind
}

func (lexer *ruleLexerImpl) Error(s string) {
	lexer.err = s
}

func (lexer *ruleLexerImpl) Result(n Rule) {
	lexer.result = n
}
