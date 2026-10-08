package rulekit

import "strings"

// PrintMode selects how a rule or AST should be printed.
type PrintMode interface {
	printMode()
}

type printSource struct{}
type printCompact struct{}
type printMultiline struct {
	indent string
}

func (printSource) printMode()    {}
func (printCompact) printMode()   {}
func (printMultiline) printMode() {}

// Source prints the original expression bytes when they are available.
func Source() PrintMode { return printSource{} }

// Compact prints compact canonical expression output.
func Compact() PrintMode { return printCompact{} }

// Multiline prints canonical multiline expression output with the given indent.
func Multiline(indent string) PrintMode {
	if indent == "" {
		indent = "  "
	}
	return printMultiline{indent: indent}
}

// Format prints an AST using an explicit print mode.
func Format(ast *AST, mode PrintMode) string {
	if ast == nil || ast.root == nil {
		return ""
	}
	switch mode := mode.(type) {
	case printSource:
		return ast.Source()
	case printMultiline:
		if tokensHaveComments(ast.tokens) {
			return formatTokensPreservingComments(ast.tokens, mode)
		}
		return formatMultiline(ast.root, mode, 0)
	default:
		if tokensHaveComments(ast.tokens) {
			return formatTokensPreservingComments(ast.tokens, printCompact{})
		}
		return printAST(ast.root)
	}
}

func tokensHaveComments(tokens []Token) bool {
	for _, tok := range tokens {
		if strings.Contains(tok.LeadingTrivia, "--") || strings.Contains(tok.LeadingTrivia, "/*") {
			return true
		}
	}
	return false
}

type tokenFormatter struct {
	b         strings.Builder
	prev      string
	depth     int
	atLine    bool
	multiline bool
	indent    string
	// groups records, for each open parenthesis, whether it groups an
	// expression (true) or encloses call arguments (false).
	groups []bool
	// spaceAfterComment separates a token from a preceding inline comment.
	spaceAfterComment bool
}

func formatTokensPreservingComments(tokens []Token, mode PrintMode) string {
	f := tokenFormatter{atLine: true}
	if mode, ok := mode.(printMultiline); ok {
		f.multiline = true
		f.indent = mode.indent
	}
	if f.indent == "" {
		f.indent = "  "
	}

	for _, tok := range tokens {
		f.writeTrivia(tok.LeadingTrivia)
		if tok.Kind == "EOF" {
			break
		}
		f.writeToken(tok)
	}
	return strings.TrimRight(f.b.String(), " \t\n")
}

func (f *tokenFormatter) writeToken(tok Token) {
	closesGroup := false
	if tok.Kind == "RPAREN" && len(f.groups) > 0 {
		closesGroup = f.groups[len(f.groups)-1]
		f.groups = f.groups[:len(f.groups)-1]
		if closesGroup && f.depth > 0 {
			f.depth--
		}
	}
	if f.multiline && (closesGroup || tok.Kind == "AND" || tok.Kind == "OR") && !f.atLine {
		f.newline()
	}

	if f.spaceAfterComment && !f.atLine && tok.Kind != "RPAREN" {
		f.b.WriteByte(' ')
	} else if f.needsSpace(tok.Kind) {
		f.b.WriteByte(' ')
	}
	f.spaceAfterComment = false

	f.b.WriteString(canonicalToken(tok))
	f.atLine = false

	if tok.Kind == "LPAREN" {
		group := f.prev != "FIELD"
		f.groups = append(f.groups, group)
		if group {
			f.depth++
			if f.multiline {
				f.newline()
			}
		}
	}
	f.prev = tok.Kind
}

func (f *tokenFormatter) writeTrivia(trivia string) {
	for len(trivia) > 0 {
		line := strings.Index(trivia, "--")
		block := strings.Index(trivia, "/*")
		idx := nextCommentIndex(line, block)
		if idx < 0 {
			return
		}
		trivia = trivia[idx:]
		if strings.HasPrefix(trivia, "--") {
			end := strings.IndexByte(trivia, '\n')
			if end < 0 {
				end = len(trivia)
			}
			comment := strings.TrimSpace(trivia[:end])
			// Compact output is single-line, so line comments become block
			// comments unless their text would end the block early.
			if text := strings.TrimSpace(comment[2:]); !f.multiline && !strings.Contains(text, "*/") {
				if text != "" {
					f.writeComment("/* " + text + " */")
				}
			} else {
				f.writeComment(comment)
				f.newline()
			}
			if end == len(trivia) {
				return
			}
			trivia = trivia[end+1:]
			continue
		}

		end := strings.Index(trivia, "*/")
		if end < 0 {
			f.writeComment(strings.TrimSpace(trivia))
			return
		}
		comment := strings.TrimSpace(trivia[:end+2])
		trivia = trivia[end+2:]
		if !f.multiline {
			f.writeComment(strings.Join(strings.Fields(comment), " "))
			continue
		}
		f.writeComment(comment)
		if strings.Contains(trivia, "\n") || strings.Contains(comment, "\n") {
			f.newline()
		}
	}
}

func (f *tokenFormatter) writeComment(comment string) {
	if comment == "" {
		return
	}
	if !f.atLine {
		f.b.WriteByte(' ')
	}
	f.b.WriteString(comment)
	f.atLine = false
	f.spaceAfterComment = true
}

func (f *tokenFormatter) newline() {
	f.b.WriteByte('\n')
	if f.multiline && f.depth > 0 {
		f.b.WriteString(strings.Repeat(f.indent, f.depth))
	}
	f.atLine = true
}

func (f *tokenFormatter) needsSpace(kind string) bool {
	if f.atLine || f.prev == "" {
		return false
	}
	if kind == "RPAREN" || kind == "RBRACKET" || kind == "COMMA" || kind == "DOT" {
		return false
	}
	if f.prev == "LPAREN" || f.prev == "LBRACKET" || f.prev == "DOT" {
		return false
	}
	if kind == "LPAREN" && f.prev == "FIELD" {
		return false
	}
	if kind == "LBRACKET" && (f.prev == "FIELD" || f.prev == "RBRACKET") {
		return false
	}
	return true
}

func canonicalToken(tok Token) string {
	switch tok.Kind {
	case "NOT":
		return "not"
	case "AND":
		return "and"
	case "OR":
		return "or"
	case "MATCHES":
		return "=~"
	case "EQ":
		return "=="
	case "NE":
		return "!="
	case "GT":
		return ">"
	case "GE":
		return ">="
	case "LT":
		return "<"
	case "LE":
		return "<="
	case "CONTAINS":
		return "contains"
	case "IN":
		return "in"
	default:
		return tok.Raw
	}
}

func nextCommentIndex(line int, block int) int {
	if line < 0 {
		return block
	}
	if block < 0 || line < block {
		return line
	}
	return block
}

func formatMultiline(node ASTNode, opts printMultiline, depth int) string {
	binary, ok := node.(*astBinary)
	if !ok || (binary.op != OperatorAnd && binary.op != OperatorOr) {
		return printAST(node)
	}
	if hasSameOperatorChild(binary) {
		return formatMultilineChain(binary, opts, depth)
	}

	left := formatMultilineOperand(binary.left, opts, depth, binary.op, false)
	right := formatMultilineOperand(binary.right, opts, depth, binary.op, true)
	operator := astOperatorString(binary.op)
	indent := strings.Repeat(opts.indent, depth)

	if leftBinary, ok := binary.left.(*astBinary); ok && astPrecedence(leftBinary) < astPrecedence(binary) {
		return left + " " + operator + " " + right
	}
	if rightBinary, ok := binary.right.(*astBinary); ok && astPrecedence(rightBinary) < astPrecedence(binary) {
		return left + " " + operator + " " + right
	}
	return left + "\n" + indent + operator + " " + right
}

func hasSameOperatorChild(binary *astBinary) bool {
	left, leftOK := binary.left.(*astBinary)
	right, rightOK := binary.right.(*astBinary)
	return leftOK && left.op == binary.op || rightOK && right.op == binary.op
}

func formatMultilineChain(binary *astBinary, opts printMultiline, depth int) string {
	operands := flattenOperator(binary, binary.op)
	operator := astOperatorString(binary.op)
	indent := strings.Repeat(opts.indent, depth)
	parts := make([]string, 0, len(operands))
	for i, operand := range operands {
		formatted := formatMultilineOperand(operand, opts, depth, binary.op, i > 0)
		if i > 0 {
			formatted = indent + operator + " " + formatted
		}
		parts = append(parts, formatted)
	}
	return strings.Join(parts, "\n")
}

func flattenOperator(node ASTNode, op Operator) []ASTNode {
	if binary, ok := node.(*astBinary); ok && binary.op == op {
		left := flattenOperator(binary.left, op)
		right := flattenOperator(binary.right, op)
		return append(left, right...)
	}
	return []ASTNode{node}
}

func formatMultilineOperand(node ASTNode, opts printMultiline, depth int, parentOp Operator, rightChild bool) string {
	if binary, ok := node.(*astBinary); ok {
		if astPrecedence(binary) < infixPrecedence(tokenKindFromASTOperator(parentOp)) {
			return formatGroupedMultiline(binary, opts, depth)
		}
		if binary.op == parentOp {
			return formatMultiline(binary, opts, depth)
		}
	}
	return printASTWithParent(node, infixPrecedence(tokenKindFromASTOperator(parentOp)), rightChild)
}

func formatGroupedMultiline(node ASTNode, opts printMultiline, depth int) string {
	inner := formatMultiline(node, opts, 0)
	innerIndent := strings.Repeat(opts.indent, depth+1)
	closingIndent := strings.Repeat(opts.indent, depth)
	return "(\n" + indentLines(inner, innerIndent) + "\n" + closingIndent + ")"
}

func indentLines(s, indent string) string {
	lines := strings.Split(s, "\n")
	for i := range lines {
		lines[i] = indent + lines[i]
	}
	return strings.Join(lines, "\n")
}
