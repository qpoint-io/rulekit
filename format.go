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

	left := printASTWithParent(binary.left, astPrecedence(binary), false)
	if leftBinary, ok := binary.left.(*astBinary); ok && leftBinary.op == binary.op {
		left = formatMultiline(binary.left, opts, depth)
	}
	right := printASTWithParent(binary.right, astPrecedence(binary), true)
	if rightBinary, ok := binary.right.(*astBinary); ok && rightBinary.op == binary.op {
		right = formatMultiline(binary.right, opts, depth)
	}

	return left + "\n" + strings.Repeat(opts.Indent, depth) + astOperatorString(binary.op) + " " + right
}
