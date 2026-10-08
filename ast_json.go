package rulekit

import (
	"encoding/json"
	"sort"
	"strconv"
)

// JSONAST is the JSON form of a parsed AST. Marshal an *AST to produce it.
type JSONAST struct {
	Source string      `json:"source"`
	Root   *JSONNode   `json:"root"`
	Tokens []JSONToken `json:"tokens"`
}

// JSONNode is the JSON form of one AST node.
//
// ID is the node's position in the tree: "root" for the root node and
// "<parent id>.<child index>" for each child. Text is the compact canonical
// expression for the node. Operator and Raw are set for unary and binary nodes
// (normalized and source spelling); Negated is true for `not contains`,
// `not matches`, and `not in`, whose Operator is the operator being negated.
// Raw is the literal token for literal nodes and the function name for call
// nodes. Path is set for path nodes.
type JSONNode struct {
	ID       string     `json:"id"`
	Kind     string     `json:"kind"`
	Text     string     `json:"text"`
	Operator string     `json:"operator,omitempty"`
	Negated  bool       `json:"negated,omitempty"`
	Raw      string     `json:"raw,omitempty"`
	Path     string     `json:"path,omitempty"`
	Span     JSONSpan   `json:"span"`
	Children []JSONNode `json:"children,omitempty"`
}

// JSONToken is the JSON form of one source token. Role groups token kinds for
// syntax highlighting: "id" (fields), "str" (string-like literals), "num"
// (numbers), "kw" (operators), or "pun" (punctuation).
type JSONToken struct {
	Kind string   `json:"kind"`
	Role string   `json:"role"`
	Raw  string   `json:"raw"`
	Span JSONSpan `json:"span"`
}

// JSONSpan is a byte span with 1-based line and column positions. Columns count
// bytes from the start of the line.
type JSONSpan struct {
	Start       int `json:"start"`
	End         int `json:"end"`
	StartLine   int `json:"startLine"`
	StartColumn int `json:"startColumn"`
	EndLine     int `json:"endLine"`
	EndColumn   int `json:"endColumn"`
}

// MarshalJSON encodes the AST as a JSONAST document.
func (a *AST) MarshalJSON() ([]byte, error) {
	return json.Marshal(a.JSON())
}

// JSON returns the JSON form of the AST. Tokens exclude the EOF token.
func (a *AST) JSON() JSONAST {
	if a == nil {
		return JSONAST{}
	}
	lines := newLineIndex(a.source)
	out := JSONAST{Source: a.source, Root: jsonNode(lines, a.Root(), "root"), Tokens: make([]JSONToken, 0, len(a.tokens))}
	for _, tok := range a.tokens {
		if tok.Kind == "EOF" {
			continue
		}
		out.Tokens = append(out.Tokens, JSONToken{Kind: tok.Kind, Role: tokenRole(tok.Kind), Raw: tok.Raw, Span: lines.span(tok.Span)})
	}
	return out
}

// JSONSpan returns the JSON form of a span in the AST source.
func (a *AST) JSONSpan(span Span) JSONSpan {
	return newLineIndex(a.Source()).span(span)
}

// String returns the JSON kind name of an AST node kind.
func (k ASTKind) String() string {
	switch k {
	case ASTLiteral:
		return "literal"
	case ASTPath:
		return "path"
	case ASTArray:
		return "array"
	case ASTCall:
		return "call"
	case ASTUnary:
		return "unary"
	case ASTBinary:
		return "binary"
	default:
		return "unknown"
	}
}

// String returns the operator's machine name, as used in JSON and
// diagnostics. Printed rules use the human spelling (==, =~, ...) instead.
func (o Operator) String() string {
	switch o {
	case OperatorNot:
		return "not"
	case OperatorAnd:
		return "and"
	case OperatorOr:
		return "or"
	case OperatorEQ:
		return "eq"
	case OperatorNE:
		return "ne"
	case OperatorGT:
		return "gt"
	case OperatorGE:
		return "ge"
	case OperatorLT:
		return "lt"
	case OperatorLE:
		return "le"
	case OperatorContains:
		return "contains"
	case OperatorMatches:
		return "matches"
	case OperatorIn:
		return "in"
	default:
		return "unknown"
	}
}

func jsonNode(lines lineIndex, node ASTNode, id string) *JSONNode {
	if node == nil {
		return nil
	}
	out := &JSONNode{ID: id, Kind: node.Kind().String(), Text: node.String(), Span: lines.span(node.Span())}
	switch n := node.(type) {
	case *astUnary:
		out.Operator, out.Raw = n.op.String(), n.rawOp
	case *astBinary:
		out.Operator, out.Negated, out.Raw = n.op.String(), n.negated, n.rawOp
	case *astLiteral:
		out.Raw = n.raw
	case *astPath:
		out.Path = (&PathValue{segments: n.segments}).String()
	case *astCall:
		out.Raw = n.name
	}
	children := node.Children()
	if len(children) > 0 {
		out.Children = make([]JSONNode, 0, len(children))
	}
	for i, child := range children {
		out.Children = append(out.Children, *jsonNode(lines, child, id+"."+strconv.Itoa(i)))
	}
	return out
}

func tokenRole(kind string) string {
	switch kind {
	case "FIELD":
		return "id"
	case "STRING", "REGEX", "IP", "IP_CIDR", "HEX_STRING", "BOOL":
		return "str"
	case "INT", "FLOAT":
		return "num"
	case "AND", "OR", "NOT", "EQ", "NE", "GT", "GE", "LT", "LE", "CONTAINS", "MATCHES", "IN":
		return "kw"
	default:
		return "pun"
	}
}

// lineIndex maps byte offsets in a source to line and column positions.
type lineIndex struct {
	starts []int // byte offset where each line starts
	size   int   // source length in bytes
}

func newLineIndex(source string) lineIndex {
	starts := []int{0}
	for i := range len(source) {
		if source[i] == '\n' {
			starts = append(starts, i+1)
		}
	}
	return lineIndex{starts: starts, size: len(source)}
}

func (l lineIndex) span(span Span) JSONSpan {
	startLine, startColumn := l.position(span.Start)
	endLine, endColumn := l.position(span.End)
	return JSONSpan{Start: span.Start, End: span.End, StartLine: startLine, StartColumn: startColumn, EndLine: endLine, EndColumn: endColumn}
}

// position returns the 1-based line and byte column of offset, clamped to the
// source bounds.
func (l lineIndex) position(offset int) (int, int) {
	offset = min(max(offset, 0), l.size)
	line := sort.Search(len(l.starts), func(i int) bool { return l.starts[i] > offset }) - 1
	return line + 1, offset - l.starts[line] + 1
}
