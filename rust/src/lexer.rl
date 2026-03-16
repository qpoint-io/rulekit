// Ragel lexer for rulekit — Rust target (Ragel 7).
// Generate with:  ragel-rust -o src/lexer.rs src/lexer.rl
// Post-process:   sed -i 's/( \(_rule_lexer_[a-z_]*\) ) as \([iu][0-9]*\)\[/\1[\2,/g' src/lexer.rs
//                 (fixes ragel 7 cast-before-index codegen bug)

use crate::types::{Token, Loc};

%%{
    machine rule_lexer;

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

    octet = digit | ( 0x31..0x39 digit ) | ( "1" digit{2} ) | ( "2" 0x30..0x34 digit ) | ( "25" 0x30..0x35 );
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

    # Regex types
    # ---

    escaped_regex_char = '\\' any;
    not_slash_or_escape = any - ('/' | '\\');
    regex_forward_slash = '/' ( not_slash_or_escape | escaped_regex_char )* '/';
    not_pipe_or_escape = any - ('|' | '\\');
    regex_pipe = '|' ( not_pipe_or_escape | escaped_regex_char )* '|';

    regex_pattern = regex_forward_slash | regex_pipe;

    # Whitespace and comments
    # ---

    ws = [ \t\n\r];
    comment_line  = '--' [^\n]* '\n'?;
    comment_block = '/*' (any - '*/')* '*/';

    field_char = alpha | digit | '_' | '.' | '-';
    field = (alpha | '_') field_char*;

    function_char = alpha | digit | '_';
    function = (alpha | '_') function_char*;

    # --- scanner logic ---

    main := |*
        # Skip comments and whitespace
        comment_line | comment_block | ws => { /* skip */ };

        # Control
        '(' => { token_kind = Lexer::TOKEN_LPAREN;   fnbreak; };
        ')' => { token_kind = Lexer::TOKEN_RPAREN;   fnbreak; };
        '[' => { token_kind = Lexer::TOKEN_LBRACKET; fnbreak; };
        ']' => { token_kind = Lexer::TOKEN_RBRACKET; fnbreak; };
        ',' => { token_kind = Lexer::TOKEN_COMMA;    fnbreak; };

        # Logical operators
        ('!' | 'not'i)  => { token_kind = Lexer::OP_NOT; fnbreak; };
        ('&&' | 'and'i) => { token_kind = Lexer::OP_AND; fnbreak; };
        ('||' | 'or'i)  => { token_kind = Lexer::OP_OR;  fnbreak; };

        # Comparison operators
        ('==' | 'eq'i) => { token_kind = Lexer::OP_EQ; fnbreak; };
        ('!=' | 'ne'i) => { token_kind = Lexer::OP_NE; fnbreak; };
        ('<' | 'lt'i)  => { token_kind = Lexer::OP_LT; fnbreak; };
        ('<=' | 'le'i) => { token_kind = Lexer::OP_LE; fnbreak; };
        ('>' | 'gt'i)  => { token_kind = Lexer::OP_GT; fnbreak; };
        ('>=' | 'ge'i) => { token_kind = Lexer::OP_GE; fnbreak; };

        'contains'i         => { token_kind = Lexer::OP_CONTAINS; fnbreak; };
        ('=~' | 'matches'i) => { token_kind = Lexer::OP_MATCHES;  fnbreak; };
        'in'i               => { token_kind = Lexer::OP_IN;       fnbreak; };

        # Values
        int    => { token_kind = Lexer::TOKEN_INT;    fnbreak; };
        float  => { token_kind = Lexer::TOKEN_FLOAT;  fnbreak; };
        bool   => { token_kind = Lexer::TOKEN_BOOL;   fnbreak; };
        string => { token_kind = Lexer::TOKEN_STRING; fnbreak; };

        ip            => { token_kind = Lexer::TOKEN_IP;         fnbreak; };
        ip_cidr       => { token_kind = Lexer::TOKEN_IP_CIDR;    fnbreak; };
        hex_string    => { token_kind = Lexer::TOKEN_HEX_STRING; fnbreak; };
        regex_pattern => { token_kind = Lexer::TOKEN_REGEX;      fnbreak; };

        function => { token_kind = Lexer::TOKEN_FUNCTION; fnbreak; };

        # Field names (allow alphanumeric and dots with restrictions)
        field => { token_kind = Lexer::TOKEN_FIELD; fnbreak; };

        # Catch any unrecognized characters
        any => {
            token_kind = Lexer::TOKEN_ERROR;
            fnbreak;
        };
    *|;

    write data nofinal;
}%%

pub struct Lexer {
    data: Vec<u8>,
    cs:   i32,
    p:    usize,
    pe:   usize,
    ts:   usize,
    te:   usize,
    act:  usize,
    eof:  usize,
}

impl std::fmt::Debug for Lexer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Lexer")
            .field("p", &self.p)
            .field("pe", &self.pe)
            .finish()
    }
}

impl Lexer {
    pub fn new(input: &[u8]) -> Self {
        let pe = input.len();
        let eof = pe;
        let mut cs: i32 = 0;
        let mut ts: usize = 0;
        let mut te: usize = 0;
        let mut act: usize = 0;

        %%write init;

        Lexer {
            data: input.to_vec(),
            cs,
            p: 0,
            pe,
            ts,
            te,
            act,
            eof,
        }
    }

    /// Return the next token. Returns token_type == -1 at EOF.
    pub fn yylex(&mut self) -> Token {
        if self.p >= self.pe {
            return Token {
                token_type: -1,
                token_value: vec![],
                loc: Loc::default(),
            };
        }

        let data: &[u8] = &self.data;
        let mut p:   usize = self.p;
        let     pe:  usize = self.pe;
        let mut cs:  i32   = self.cs;
        let mut ts:  usize = self.ts;
        let mut te:  usize = self.te;
        let mut act: usize = self.act;
        let     eof: usize = self.eof;
        let mut token_kind: i32 = 0;

        %%write exec;

        self.p   = p;
        self.cs  = cs;
        self.ts  = ts;
        self.te  = te;
        self.act = act;

        if token_kind == 0 {
            // Consumed whitespace/comments — try again.
            if p < pe {
                return self.yylex();
            }
            return Token {
                token_type: -1,
                token_value: vec![],
                loc: Loc::default(),
            };
        }

        let raw = if ts <= te && te <= data.len() {
            data[ts..te].to_vec()
        } else {
            vec![]
        };

        Token {
            token_type: token_kind,
            token_value: raw,
            loc: Loc { begin: ts, end: te },
        }
    }
}
