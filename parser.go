package rulekit

import (
	"fmt"
	"io"
	"net"
	"os"
	"regexp"
	"strconv"
	"strings"
	"unicode"
	"unicode/utf8"
)

const (
	token_EOF = iota
	token_ERROR
	token_FIELD
	token_STRING
	token_HEX_STRING
	token_INT
	token_FLOAT
	token_BOOL
	token_IP_CIDR
	token_IP
	token_REGEX
	token_LPAREN
	token_RPAREN
	token_LBRACKET
	token_RBRACKET
	token_DOT
	token_COMMA

	op_NOT
	op_AND
	op_OR
	op_EQ
	op_NE
	op_GT
	op_GE
	op_LT
	op_LE
	op_CONTAINS
	op_MATCHES
	op_IN
)

var (
	ruleDebug       int
	ruleDebugWriter io.Writer = os.Stderr
)

func init() {
	SetErrorVerbose(true)
}

// SetDebugLevel sets the debug verbosity level.
func SetDebugLevel(level int) {
	ruleDebug = level
}

func SetDebugWriter(w io.Writer) {
	ruleDebugWriter = w
}

// SetErrorVerbose is retained for API compatibility. The hand-written parser
// always returns verbose ParseError values.
func SetErrorVerbose(bool) {}

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
		str = str[1 : len(str)-1]
		str = strings.ReplaceAll(str, `"`, `\"`)
		str = strings.ReplaceAll(str, `\'`, `'`)
		str = `"` + str + `"`
	}
	var err error
	str, err = strconv.Unquote(str)
	if err != nil {
		return nil, err
	}

	if ip := net.ParseIP(str); ip != nil {
		return ip, nil
	} else if _, ipnet, err := net.ParseCIDR(str); err == nil {
		return ipnet, nil
	} else if strings.Count(str, ":") == 5 || strings.Count(str, ":") == 7 {
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
	pattern := raw[1 : len(raw)-1]
	return regexp.Compile(pattern)
}

func parseValueToken(typ int, rawBytes []byte) (Rule, error) {
	raw := string(rawBytes)
	var (
		value any
		err   error
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
		return nil, ValueParseError{typ, raw, err}
	}
	return &LiteralValue[any]{raw: raw, value: value}, nil
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
	case token_DOT:
		return `"."`
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

type token struct {
	kind       int
	raw        string
	start, end int
}

type lexer struct {
	input string
	pos   int
}

func lex(input string) ([]token, error) {
	l := &lexer{input: input}
	var tokens []token
	for {
		tok := l.next()
		tokens = append(tokens, tok)
		if tok.kind == token_EOF {
			return tokens, nil
		}
		if tok.kind == token_ERROR {
			return tokens, fmt.Errorf("%s", tok.raw)
		}
	}
}

func (l *lexer) next() token {
	if err := l.skipIgnored(); err != nil {
		return token{kind: token_ERROR, raw: err.Error(), start: l.pos, end: l.pos}
	}
	if l.pos >= len(l.input) {
		return token{kind: token_EOF, start: l.pos, end: l.pos}
	}

	start := l.pos
	ch := l.input[l.pos]
	switch ch {
	case '(':
		l.pos++
		return token{kind: token_LPAREN, raw: "(", start: start, end: l.pos}
	case ')':
		l.pos++
		return token{kind: token_RPAREN, raw: ")", start: start, end: l.pos}
	case '[':
		l.pos++
		return token{kind: token_LBRACKET, raw: "[", start: start, end: l.pos}
	case ']':
		l.pos++
		return token{kind: token_RBRACKET, raw: "]", start: start, end: l.pos}
	case ',':
		l.pos++
		return token{kind: token_COMMA, raw: ",", start: start, end: l.pos}
	case '.':
		l.pos++
		return token{kind: token_DOT, raw: ".", start: start, end: l.pos}
	case '!':
		if l.match("!=") {
			return token{kind: op_NE, raw: "!=", start: start, end: l.pos}
		}
		l.pos++
		return token{kind: op_NOT, raw: "!", start: start, end: l.pos}
	case '&':
		if l.match("&&") {
			return token{kind: op_AND, raw: "&&", start: start, end: l.pos}
		}
	case '|':
		if l.match("||") {
			return token{kind: op_OR, raw: "||", start: start, end: l.pos}
		}
		return l.scanDelimited('|', token_REGEX)
	case '=':
		if l.match("==") {
			return token{kind: op_EQ, raw: "==", start: start, end: l.pos}
		}
		if l.match("=~") {
			return token{kind: op_MATCHES, raw: "=~", start: start, end: l.pos}
		}
	case '<':
		if l.match("<=") {
			return token{kind: op_LE, raw: "<=", start: start, end: l.pos}
		}
		l.pos++
		return token{kind: op_LT, raw: "<", start: start, end: l.pos}
	case '>':
		if l.match(">=") {
			return token{kind: op_GE, raw: ">=", start: start, end: l.pos}
		}
		l.pos++
		return token{kind: op_GT, raw: ">", start: start, end: l.pos}
	case '/', '\'', '"':
		if ch == '/' && l.hasPrefix("/*") {
			break
		}
		kind := token_REGEX
		if ch == '\'' || ch == '"' {
			kind = token_STRING
		}
		return l.scanDelimited(ch, kind)
	}

	if ch == '+' || ch == '-' || ch == ':' || isAtomStart(rune(ch)) || unicode.IsDigit(rune(ch)) {
		return l.scanAtom()
	}

	l.pos++
	return token{kind: token_ERROR, raw: fmt.Sprintf("unexpected character: %q", l.input[start:l.pos]), start: start, end: l.pos}
}

func (l *lexer) skipIgnored() error {
	for l.pos < len(l.input) {
		r, size := utf8.DecodeRuneInString(l.input[l.pos:])
		if unicode.IsSpace(r) {
			l.pos += size
			continue
		}
		if l.hasPrefix("--") {
			l.pos += 2
			for l.pos < len(l.input) && l.input[l.pos] != '\n' {
				l.pos++
			}
			continue
		}
		if l.hasPrefix("/*") {
			end := strings.Index(l.input[l.pos+2:], "*/")
			if end < 0 {
				return fmt.Errorf("unterminated block comment")
			}
			l.pos += end + 4
			continue
		}
		break
	}
	return nil
}

func (l *lexer) scanDelimited(delim byte, kind int) token {
	start := l.pos
	l.pos++
	escaped := false
	for l.pos < len(l.input) {
		ch := l.input[l.pos]
		l.pos++
		if escaped {
			escaped = false
			continue
		}
		if ch == '\\' {
			escaped = true
			continue
		}
		if ch == delim {
			return token{kind: kind, raw: l.input[start:l.pos], start: start, end: l.pos}
		}
	}
	return token{kind: token_ERROR, raw: "unterminated literal", start: start, end: l.pos}
}

func (l *lexer) scanAtom() token {
	start := l.pos
	for l.pos < len(l.input) {
		ch := l.input[l.pos]
		if unicode.IsSpace(rune(ch)) || strings.ContainsRune("()[],<>=!&|\"'", rune(ch)) {
			break
		}
		l.pos++
	}
	raw := l.input[start:l.pos]
	lower := strings.ToLower(raw)
	switch lower {
	case "not":
		return token{kind: op_NOT, raw: raw, start: start, end: l.pos}
	case "and":
		return token{kind: op_AND, raw: raw, start: start, end: l.pos}
	case "or":
		return token{kind: op_OR, raw: raw, start: start, end: l.pos}
	case "eq":
		return token{kind: op_EQ, raw: raw, start: start, end: l.pos}
	case "ne":
		return token{kind: op_NE, raw: raw, start: start, end: l.pos}
	case "gt":
		return token{kind: op_GT, raw: raw, start: start, end: l.pos}
	case "ge":
		return token{kind: op_GE, raw: raw, start: start, end: l.pos}
	case "lt":
		return token{kind: op_LT, raw: raw, start: start, end: l.pos}
	case "le":
		return token{kind: op_LE, raw: raw, start: start, end: l.pos}
	case "contains":
		return token{kind: op_CONTAINS, raw: raw, start: start, end: l.pos}
	case "matches":
		return token{kind: op_MATCHES, raw: raw, start: start, end: l.pos}
	case "in":
		return token{kind: op_IN, raw: raw, start: start, end: l.pos}
	case "true", "false":
		return token{kind: token_BOOL, raw: raw, start: start, end: l.pos}
	}

	if _, _, err := net.ParseCIDR(raw); err == nil {
		return token{kind: token_IP_CIDR, raw: raw, start: start, end: l.pos}
	}
	if net.ParseIP(raw) != nil {
		return token{kind: token_IP, raw: raw, start: start, end: l.pos}
	}
	if isInteger(raw) {
		return token{kind: token_INT, raw: raw, start: start, end: l.pos}
	}
	if isFloat(raw) {
		return token{kind: token_FLOAT, raw: raw, start: start, end: l.pos}
	}
	if isHexString(raw) {
		return token{kind: token_HEX_STRING, raw: raw, start: start, end: l.pos}
	}
	if isField(raw) {
		return token{kind: token_FIELD, raw: raw, start: start, end: l.pos}
	}
	return token{kind: token_ERROR, raw: fmt.Sprintf("unexpected token: %q", raw), start: start, end: l.pos}
}

func (l *lexer) match(s string) bool {
	if !l.hasPrefix(s) {
		return false
	}
	l.pos += len(s)
	return true
}

func (l *lexer) hasPrefix(s string) bool {
	return strings.HasPrefix(l.input[l.pos:], s)
}

func isAtomStart(r rune) bool {
	return unicode.IsLetter(r) || r == '_'
}

func isField(s string) bool {
	if s == "" {
		return false
	}
	for i, r := range s {
		if i == 0 {
			if !isAtomStart(r) {
				return false
			}
			continue
		}
		if !unicode.IsLetter(r) && !unicode.IsDigit(r) && r != '_' && r != '.' && r != '-' {
			return false
		}
	}
	return true
}

func isInteger(s string) bool {
	if s == "" || s == "+" || s == "-" {
		return false
	}
	_, err := strconv.ParseInt(s, 0, 64)
	if err == nil {
		return true
	}
	_, err = strconv.ParseUint(s, 0, 64)
	return err == nil
}

func isFloat(s string) bool {
	if !strings.Contains(s, ".") {
		return false
	}
	_, err := strconv.ParseFloat(s, 64)
	return err == nil
}

func isHexString(s string) bool {
	if len(s) < 2 {
		return false
	}
	parts := strings.Split(s, ":")
	for _, part := range parts {
		if len(part) != 2 {
			return false
		}
		for _, r := range part {
			if !unicode.IsDigit(r) && (unicode.ToLower(r) < 'a' || unicode.ToLower(r) > 'f') {
				return false
			}
		}
	}
	return true
}

type parser struct {
	input  string
	tokens []token
	pos    int
}

func parseRule(input string) (Rule, error) {
	expr, err := parseAST(input)
	if err != nil {
		return nil, err
	}
	rule, err := lowerAST(expr)
	if err != nil {
		if lowerErr, ok := err.(*astLowerError); ok {
			return nil, newParseError(input, token{start: lowerErr.span.Start, end: lowerErr.span.End}, lowerErr.msg)
		}
		return nil, err
	}
	return rule, nil
}

func parseAST(input string) (astNode, error) {
	tokens, err := lex(input)
	if err != nil {
		return nil, newParseError(input, tokens[len(tokens)-1], err.Error())
	}
	p := &parser{input: input, tokens: tokens}
	expr, err := p.parseExpr(0)
	if err != nil {
		return nil, err
	}
	if tok := p.peek(); tok.kind != token_EOF {
		return nil, p.errorf(tok, "unexpected token %q", tok.raw)
	}
	return expr, nil
}

func (p *parser) parseExpr(minPrec int) (astNode, error) {
	left, err := p.parsePrimary()
	if err != nil {
		return nil, err
	}
	left, err = p.parsePostfix(left)
	if err != nil {
		return nil, err
	}

	for {
		tok := p.peek()
		prec := infixPrecedence(tok.kind)
		if prec < minPrec {
			break
		}
		p.next()

		switch tok.kind {
		case op_AND, op_OR:
			right, err := p.parseExpr(prec + 1)
			if err != nil {
				return nil, err
			}
			left = &astBinary{span: joinSpan(left.astSpan(), right.astSpan()), left: left, op: astOperatorFromToken(tok.kind), rawOp: tok.raw, right: right}
		case op_EQ, op_NE, op_CONTAINS, op_GT, op_GE, op_LT, op_LE:
			right, err := p.parseExpr(prec + 1)
			if err != nil {
				return nil, err
			}
			if isInequality(tok.kind) && (!astValidInequalityOperand(left) || !astValidInequalityOperand(right)) {
				return nil, p.errorf(tok, "invalid operation")
			}
			left = &astBinary{span: joinSpan(left.astSpan(), right.astSpan()), left: left, op: astOperatorFromToken(tok.kind), rawOp: tok.raw, right: right}
		case op_MATCHES:
			rhs := p.peek()
			if rhs.kind != token_REGEX {
				return nil, p.errorf(rhs, "matches requires a regex value")
			}
			right, err := p.parsePrimary()
			if err != nil {
				return nil, err
			}
			right, err = p.parsePostfix(right)
			if err != nil {
				return nil, err
			}
			left = &astBinary{span: joinSpan(left.astSpan(), right.astSpan()), left: left, op: astOpMatches, rawOp: tok.raw, right: right}
		case op_IN:
			right, err := p.parseExpr(prec + 1)
			if err != nil {
				return nil, err
			}
			if !astLiteralIs(right, token_IP_CIDR) {
				if _, ok := right.(*astArray); !ok {
					return nil, p.errorf(tok, "in requires an array or CIDR value")
				}
			}
			left = &astBinary{span: joinSpan(left.astSpan(), right.astSpan()), left: left, op: astOpIn, rawOp: tok.raw, right: right}
		}
	}
	return left, nil
}

func (p *parser) parsePrimary() (astNode, error) {
	tok := p.next()
	switch tok.kind {
	case token_FIELD:
		if p.peek().kind == token_LPAREN {
			return p.parseFunction(tok)
		}
		return &astPath{span: spanFromToken(tok), segments: fieldPathSegments(tok.raw)}, nil
	case token_STRING, token_INT, token_FLOAT, token_BOOL, token_IP, token_IP_CIDR, token_HEX_STRING, token_REGEX:
		return &astLiteral{span: spanFromToken(tok), kind: tok.kind, raw: tok.raw}, nil
	case token_LPAREN:
		expr, err := p.parseExpr(0)
		if err != nil {
			return nil, err
		}
		if _, err := p.expect(token_RPAREN); err != nil {
			return nil, err
		}
		return expr, nil
	case token_LBRACKET:
		if p.isRootBracketPath() {
			return p.parseRootBracketPath(tok)
		}
		return p.parseArray(tok)
	case op_NOT:
		right, err := p.parseExpr(4)
		if err != nil {
			return nil, err
		}
		return &astUnary{span: joinSpan(spanFromToken(tok), right.astSpan()), op: astOpNot, rawOp: tok.raw, right: right}, nil
	case token_EOF:
		return nil, p.errorf(tok, "empty expression")
	default:
		return nil, p.errorf(tok, "unexpected token %q", tok.raw)
	}
}

func (p *parser) parsePostfix(left astNode) (astNode, error) {
	for {
		switch p.peek().kind {
		case token_LBRACKET:
			start := p.next()
			seg, err := p.parseBracketSegment(start)
			if err != nil {
				return nil, err
			}
			path, ok := asASTPath(left)
			if !ok {
				return nil, p.errorf(start, "bracket indexing requires a field path")
			}
			path.segments = append(path.segments, seg)
			path.span.End = p.tokens[p.pos-1].end
			left = path
		case token_DOT:
			dot := p.next()
			tok, err := p.expect(token_FIELD)
			if err != nil {
				return nil, err
			}
			path, ok := asASTPath(left)
			if !ok {
				return nil, p.errorf(dot, "dot traversal requires a field path")
			}
			path.segments = append(path.segments, fieldPathSegments(tok.raw)...)
			path.span.End = tok.end
			left = path
		default:
			return left, nil
		}
	}
}

func (p *parser) isRootBracketPath() bool {
	if p.pos+2 >= len(p.tokens) {
		return false
	}
	if p.tokens[p.pos].kind != token_STRING || p.tokens[p.pos+1].kind != token_RBRACKET {
		return false
	}
	switch p.tokens[p.pos+2].kind {
	case token_LBRACKET, token_DOT, op_EQ, op_NE, op_GT, op_GE, op_LT, op_LE, op_CONTAINS, op_MATCHES, op_IN:
		return true
	default:
		return false
	}
}

func (p *parser) parseRootBracketPath(start token) (astNode, error) {
	seg, err := p.parseBracketSegment(start)
	if err != nil {
		return nil, err
	}
	return &astPath{span: astSpan{Start: start.start, End: p.tokens[p.pos-1].end}, segments: []pathSegment{seg}}, nil
}

func (p *parser) parseBracketSegment(start token) (pathSegment, error) {
	tok := p.next()
	var seg pathSegment
	switch tok.kind {
	case token_STRING:
		key, err := parsePathKey(tok.raw)
		if err != nil {
			return pathSegment{}, p.errorf(tok, "%s", err.Error())
		}
		if key == "" {
			return pathSegment{}, p.errorf(tok, "bracket key must not be empty")
		}
		seg = pathSegment{key: key, bracket: true}
	case token_INT:
		if strings.HasPrefix(tok.raw, "+") || strings.HasPrefix(tok.raw, "-") {
			return pathSegment{}, p.errorf(tok, "array index must be an unsigned integer")
		}
		idx, err := strconv.ParseUint(tok.raw, 10, 0)
		if err != nil {
			return pathSegment{}, p.errorf(tok, "array index must be an unsigned integer")
		}
		seg = pathSegment{index: int(idx), isIndex: true, bracket: true}
	default:
		return pathSegment{}, p.errorf(tok, "bracket key must be a quoted string or unsigned integer")
	}
	if _, err := p.expect(token_RBRACKET); err != nil {
		return pathSegment{}, err
	}
	_ = start
	return seg, nil
}

func (p *parser) parseArray(start token) (astNode, error) {
	if p.peek().kind == token_RBRACKET {
		return nil, p.errorf(p.peek(), "array requires at least one value")
	}
	var vals []astNode
	for {
		val, err := p.parseArrayValue()
		if err != nil {
			return nil, err
		}
		vals = append(vals, val)
		if p.peek().kind != token_COMMA {
			break
		}
		p.next()
		if p.peek().kind == token_RBRACKET {
			return nil, p.errorf(p.peek(), "trailing commas are not allowed")
		}
	}
	if _, err := p.expect(token_RBRACKET); err != nil {
		return nil, err
	}
	_ = start
	return &astArray{span: astSpan{Start: start.start, End: p.tokens[p.pos-1].end}, vals: vals}, nil
}

func (p *parser) parseArrayValue() (astNode, error) {
	tok := p.peek()
	switch tok.kind {
	case token_LBRACKET:
		return nil, p.errorf(tok, "nested arrays are not allowed")
	case token_FIELD, token_STRING, token_INT, token_FLOAT, token_BOOL, token_IP, token_IP_CIDR, token_HEX_STRING, token_REGEX:
		val, err := p.parsePrimary()
		if err != nil {
			return nil, err
		}
		return p.parsePostfix(val)
	default:
		return nil, p.errorf(tok, "array values must be literals or fields")
	}
}

func (p *parser) parseFunction(name token) (astNode, error) {
	p.next()
	var args []astNode
	if p.peek().kind != token_RPAREN {
		for {
			arg, err := p.parseExpr(0)
			if err != nil {
				return nil, err
			}
			args = append(args, arg)
			if p.peek().kind != token_COMMA {
				break
			}
			p.next()
		}
	}
	end, err := p.expect(token_RPAREN)
	if err != nil {
		return nil, err
	}
	if stdlibFn, ok := StdlibFuncs[name.raw]; ok && len(stdlibFn.Args) != len(args) {
		err := fmt.Errorf("function %q expects %d arguments, got %d", name.raw, len(stdlibFn.Args), len(args))
		return nil, p.errorf(token{start: end.end, end: end.end}, "%s", err.Error())
	}
	return &astCall{span: astSpan{Start: name.start, End: end.end}, name: name.raw, args: args}, nil
}

func asASTPath(node astNode) (*astPath, bool) {
	path, ok := node.(*astPath)
	return path, ok
}

func (p *parser) expect(kind int) (token, error) {
	tok := p.next()
	if tok.kind != kind {
		return tok, p.errorf(tok, "expected %s", valueTokenString(kind))
	}
	return tok, nil
}

func (p *parser) peek() token {
	return p.tokens[p.pos]
}

func (p *parser) next() token {
	tok := p.tokens[p.pos]
	if p.pos < len(p.tokens)-1 {
		p.pos++
	}
	return tok
}

func (p *parser) errorf(tok token, format string, args ...any) error {
	return newParseError(p.input, tok, fmt.Sprintf(format, args...))
}

func infixPrecedence(kind int) int {
	switch kind {
	case op_OR:
		return 1
	case op_AND:
		return 2
	case op_EQ, op_NE, op_CONTAINS, op_GT, op_GE, op_LT, op_LE, op_MATCHES, op_IN:
		return 3
	default:
		return -1
	}
}

func isInequality(op int) bool {
	return op == op_GT || op == op_GE || op == op_LT || op == op_LE
}

func validInequalityOperand(r Rule) bool {
	if _, ok := r.(FieldValue); ok {
		return true
	}
	if _, ok := r.(*PathValue); ok {
		return true
	}
	if _, ok := r.(*FunctionValue); ok {
		return true
	}
	if literalIs[int64](r) || literalIs[uint64](r) || literalIs[float64](r) {
		return true
	}
	return false
}

func literalIs[T any](r Rule) bool {
	lit, ok := r.(*LiteralValue[any])
	if !ok {
		return false
	}
	_, ok = lit.value.(T)
	return ok
}

func newParseError(input string, tok token, message string) *ParseError {
	line, col := getLineColumn(input, tok.start)
	return &ParseError{
		Line:       line,
		Column:     col,
		Message:    message,
		Input:      input,
		Suggestion: getSuggestion(message),
	}
}

// Helper function to get line and column from byte position.
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

func getSuggestion(err string) string {
	switch {
	case strings.Contains(err, "parsing string"):
		return "string values must be properly quoted with matching quotes (e.g. \"hello\")"
	case strings.Contains(err, "parsing integer"):
		return "integer values must be valid integers without decimals (e.g. 42)"
	case strings.Contains(err, "parsing float"):
		return "floating-point numbers must be in the format 1.23"
	case strings.Contains(err, "parsing boolean"):
		return "boolean values must be either 'true' or 'false' (case insensitive)"
	case strings.Contains(err, "parsing IP"):
		return "IP addresses must be in valid IPv4 (e.g. 192.168.1.1) or IPv6 format"
	case strings.Contains(err, "parsing CIDR"):
		return "CIDR blocks must be in valid format (e.g. 192.168.1.0/24)"
	case strings.Contains(err, "parsing hex"):
		return "hex strings must contain valid hex digits optionally separated by colons"
	case strings.Contains(err, "regex"):
		return "regex patterns must be surrounded by / or | and contain valid regex syntax"
	case strings.Contains(err, "field"):
		return "field names must be valid identifiers (e.g. 'field_name' or 'field.name')"
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
