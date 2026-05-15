package rulekit

import (
	"fmt"
	"strings"
)

// FormatMode selects canonical output shape.
type FormatMode int

const (
	FormatCompact FormatMode = iota
	FormatMultiline
)

// FormatOptions configures expression formatting.
type FormatOptions struct {
	Mode   FormatMode
	Indent string
}

// Format prints an AST using explicit canonical formatting options.
func Format(ast *AST, opts FormatOptions) (string, error) {
	if ast == nil || ast.root == nil {
		return "", fmt.Errorf("AST must not be nil")
	}
	if opts.Indent == "" {
		opts.Indent = "  "
	}
	switch opts.Mode {
	case FormatCompact:
		return printAST(ast.root), nil
	case FormatMultiline:
		return formatMultiline(ast.root, opts, 0), nil
	default:
		return "", fmt.Errorf("unknown format mode %d", opts.Mode)
	}
}

func formatMultiline(node astNode, opts FormatOptions, depth int) string {
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
	indent := strings.Repeat(opts.Indent, depth)

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

func formatMultilineChain(binary *astBinary, opts FormatOptions, depth int) string {
	operands := flattenOperator(binary, binary.op)
	operator := astOperatorString(binary.op)
	indent := strings.Repeat(opts.Indent, depth)
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

func formatMultilineOperand(node astNode, opts FormatOptions, depth int, parentOp astOperator, rightChild bool) string {
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

func formatGroupedMultiline(node astNode, opts FormatOptions, depth int) string {
	inner := formatMultiline(node, opts, 0)
	innerIndent := strings.Repeat(opts.Indent, depth+1)
	closingIndent := strings.Repeat(opts.Indent, depth)
	return "(\n" + indentLines(inner, innerIndent) + "\n" + closingIndent + ")"
}

func indentLines(s, indent string) string {
	lines := strings.Split(s, "\n")
	for i := range lines {
		lines[i] = indent + lines[i]
	}
	return strings.Join(lines, "\n")
}
