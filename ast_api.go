package rulekit

import "fmt"

// Span identifies a byte range in the original expression.
type Span struct {
	Start int
	End   int
}

// AST is a parsed expression tree. The current public AST is immutable; callers
// that edit expressions should parse, inspect, and compile replacement trees.
type AST struct {
	source string
	root   astNode
	tokens []Token
}

// Token is a lossless lexical token with attached trivia.
type Token struct {
	Kind           string
	Raw            string
	Span           Span
	LeadingTrivia  string
	TrailingTrivia string
}

// ASTKind identifies the shape of an AST node.
type ASTKind int

const (
	ASTUnknown ASTKind = iota
	ASTLiteral
	ASTPath
	ASTArray
	ASTCall
	ASTUnary
	ASTBinary
)

// Operator identifies normalized unary and binary operators.
type Operator int

const (
	OperatorUnknown Operator = iota
	OperatorNot
	OperatorAnd
	OperatorOr
	OperatorEQ
	OperatorNE
	OperatorGT
	OperatorGE
	OperatorLT
	OperatorLE
	OperatorContains
	OperatorMatches
	OperatorIn
)

// ASTNode is the read-only public view of a parsed expression node.
type ASTNode interface {
	Kind() ASTKind
	Span() Span
	String() string
	Children() []ASTNode
}

// ParseAST parses an expression and returns its editable-source AST boundary.
func ParseAST(expr string) (*AST, error) {
	root, tokens, err := parseASTWithTokens(expr)
	if err != nil {
		return nil, err
	}
	return &AST{source: expr, root: root, tokens: publicTokens(tokens)}, nil
}

// Compile lowers a parsed AST to the evaluator Rule representation.
func Compile(ast *AST) (Rule, error) {
	if ast == nil || ast.root == nil {
		return nil, fmt.Errorf("AST must not be nil")
	}
	lowered, err := lowerAST(ast.root)
	if err != nil {
		if lowerErr, ok := err.(*astLowerError); ok {
			return nil, newParseError(ast.source, token{start: lowerErr.span.Start, end: lowerErr.span.End}, lowerErr.msg)
		}
		return nil, err
	}
	return &rule{Rule: lowered, ast: ast}, nil
}

// Root returns the root expression node.
func (a *AST) Root() ASTNode {
	if a == nil {
		return nil
	}
	return astToPublic(a.root)
}

// Source returns the expression text used to build the AST.
func (a *AST) Source() string {
	if a == nil {
		return ""
	}
	return a.source
}

// Tokens returns the parsed token stream, including the EOF token.
func (a *AST) Tokens() []Token {
	if a == nil {
		return nil
	}
	tokens := make([]Token, len(a.tokens))
	copy(tokens, a.tokens)
	return tokens
}

// String returns the compact canonical AST expression.
func (a *AST) String() string {
	if a == nil || a.root == nil {
		return ""
	}
	return printAST(a.root)
}

// NodeOperator returns a normalized operator for unary and binary nodes.
func NodeOperator(node ASTNode) Operator {
	switch n := node.(type) {
	case *astUnary:
		return publicOperator(n.op)
	case *astBinary:
		return publicOperator(n.op)
	default:
		return OperatorUnknown
	}
}

// NodeRawOperator returns the source operator spelling for unary and binary nodes.
func NodeRawOperator(node ASTNode) string {
	switch n := node.(type) {
	case *astUnary:
		return n.rawOp
	case *astBinary:
		return n.rawOp
	default:
		return ""
	}
}

// NodeLiteral returns the raw literal token for literal nodes.
func NodeLiteral(node ASTNode) (raw string, ok bool) {
	lit, ok := node.(*astLiteral)
	if !ok {
		return "", false
	}
	return lit.raw, true
}

// NodePath returns path segments for path nodes.
func NodePath(node ASTNode) ([]PathSegment, bool) {
	path, ok := node.(*astPath)
	if !ok {
		return nil, false
	}
	segments := make([]PathSegment, 0, len(path.segments))
	for _, seg := range path.segments {
		segments = append(segments, PathSegment{Key: seg.key, Index: seg.index, IsIndex: seg.isIndex, Bracket: seg.bracket})
	}
	return segments, true
}

// NodeCallName returns the function or macro name for call nodes.
func NodeCallName(node ASTNode) (string, bool) {
	call, ok := node.(*astCall)
	if !ok {
		return "", false
	}
	return call.name, true
}

// PathSegment is a public read-only path segment value.
type PathSegment struct {
	Key     string
	Index   int
	IsIndex bool
	Bracket bool
}

func publicOperator(op astOperator) Operator {
	switch op {
	case astOpNot:
		return OperatorNot
	case astOpAnd:
		return OperatorAnd
	case astOpOr:
		return OperatorOr
	case astOpEQ:
		return OperatorEQ
	case astOpNE:
		return OperatorNE
	case astOpGT:
		return OperatorGT
	case astOpGE:
		return OperatorGE
	case astOpLT:
		return OperatorLT
	case astOpLE:
		return OperatorLE
	case astOpContains:
		return OperatorContains
	case astOpMatches:
		return OperatorMatches
	case astOpIn:
		return OperatorIn
	default:
		return OperatorUnknown
	}
}

func publicSpan(span astSpan) Span {
	return Span{Start: span.Start, End: span.End}
}

func publicTokens(tokens []token) []Token {
	out := make([]Token, 0, len(tokens))
	for _, tok := range tokens {
		out = append(out, Token{
			Kind:           tokenKindString(tok.kind),
			Raw:            tok.raw,
			Span:           Span{Start: tok.start, End: tok.end},
			LeadingTrivia:  tok.leadingTrivia,
			TrailingTrivia: tok.trailingTrivia,
		})
	}
	return out
}

func tokenKindString(kind int) string {
	switch kind {
	case token_EOF:
		return "EOF"
	case token_ERROR:
		return "ERROR"
	case op_NOT:
		return "NOT"
	case op_AND:
		return "AND"
	case op_OR:
		return "OR"
	case op_EQ:
		return "EQ"
	case op_NE:
		return "NE"
	case op_GT:
		return "GT"
	case op_GE:
		return "GE"
	case op_LT:
		return "LT"
	case op_LE:
		return "LE"
	case op_CONTAINS:
		return "CONTAINS"
	case op_MATCHES:
		return "MATCHES"
	case op_IN:
		return "IN"
	default:
		return valueTokenString(kind)
	}
}

func publicChildren(children ...astNode) []ASTNode {
	out := make([]ASTNode, 0, len(children))
	for _, child := range children {
		if child != nil {
			out = append(out, astToPublic(child))
		}
	}
	return out
}

func publicChildSlice(children []astNode) []ASTNode {
	out := make([]ASTNode, 0, len(children))
	for _, child := range children {
		out = append(out, astToPublic(child))
	}
	return out
}

func astToPublic(node astNode) ASTNode {
	public, _ := node.(ASTNode)
	return public
}
