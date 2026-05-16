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
		return formatMultiline(ast.root, formatOptions{indent: mode.indent}, 0)
	default:
		return printAST(ast.root)
	}
}

type formatOptions struct {
	indent string
}

func formatMultiline(node astNode, opts formatOptions, depth int) string {
	binary, ok := node.(*astBinary)
	if !ok || (binary.op != astOpAnd && binary.op != astOpOr) {
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

func formatMultilineChain(binary *astBinary, opts formatOptions, depth int) string {
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

func flattenOperator(node astNode, op astOperator) []astNode {
	if binary, ok := node.(*astBinary); ok && binary.op == op {
		left := flattenOperator(binary.left, op)
		right := flattenOperator(binary.right, op)
		return append(left, right...)
	}
	return []astNode{node}
}

func formatMultilineOperand(node astNode, opts formatOptions, depth int, parentOp astOperator, rightChild bool) string {
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

func formatGroupedMultiline(node astNode, opts formatOptions, depth int) string {
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
