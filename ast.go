package rulekit

import (
	"net"
	"strings"
)

type astSpan struct {
	Start int
	End   int
}

func spanFromToken(tok token) astSpan {
	return astSpan{Start: tok.start, End: tok.end}
}

func joinSpan(left, right astSpan) astSpan {
	if left.Start == 0 && left.End == 0 {
		return right
	}
	if right.Start == 0 && right.End == 0 {
		return left
	}
	return astSpan{Start: left.Start, End: right.End}
}

type astOperator int

const (
	astOpUnknown astOperator = iota
	astOpNot
	astOpAnd
	astOpOr
	astOpEQ
	astOpNE
	astOpGT
	astOpGE
	astOpLT
	astOpLE
	astOpContains
	astOpMatches
	astOpIn
)

func astOperatorFromToken(kind int) astOperator {
	switch kind {
	case op_NOT:
		return astOpNot
	case op_AND:
		return astOpAnd
	case op_OR:
		return astOpOr
	case op_EQ:
		return astOpEQ
	case op_NE:
		return astOpNE
	case op_GT:
		return astOpGT
	case op_GE:
		return astOpGE
	case op_LT:
		return astOpLT
	case op_LE:
		return astOpLE
	case op_CONTAINS:
		return astOpContains
	case op_MATCHES:
		return astOpMatches
	case op_IN:
		return astOpIn
	default:
		return astOpUnknown
	}
}

func tokenKindFromASTOperator(op astOperator) int {
	switch op {
	case astOpNot:
		return op_NOT
	case astOpAnd:
		return op_AND
	case astOpOr:
		return op_OR
	case astOpEQ:
		return op_EQ
	case astOpNE:
		return op_NE
	case astOpGT:
		return op_GT
	case astOpGE:
		return op_GE
	case astOpLT:
		return op_LT
	case astOpLE:
		return op_LE
	case astOpContains:
		return op_CONTAINS
	case astOpMatches:
		return op_MATCHES
	case astOpIn:
		return op_IN
	default:
		return 0
	}
}

type astNode interface {
	astSpan() astSpan
}

type astLiteral struct {
	span astSpan
	kind int
	raw  string
}

func (n *astLiteral) astSpan() astSpan { return n.span }
func (n *astLiteral) Kind() ASTKind    { return ASTLiteral }
func (n *astLiteral) Span() Span       { return publicSpan(n.span) }
func (n *astLiteral) String() string   { return printAST(n) }
func (n *astLiteral) Children() []ASTNode {
	return nil
}

type astPath struct {
	span     astSpan
	segments []pathSegment
}

func (n *astPath) astSpan() astSpan { return n.span }
func (n *astPath) Kind() ASTKind    { return ASTPath }
func (n *astPath) Span() Span       { return publicSpan(n.span) }
func (n *astPath) String() string   { return printAST(n) }
func (n *astPath) Children() []ASTNode {
	return nil
}

type astArray struct {
	span astSpan
	vals []astNode
}

func (n *astArray) astSpan() astSpan { return n.span }
func (n *astArray) Kind() ASTKind    { return ASTArray }
func (n *astArray) Span() Span       { return publicSpan(n.span) }
func (n *astArray) String() string   { return printAST(n) }
func (n *astArray) Children() []ASTNode {
	return publicChildSlice(n.vals)
}

type astCall struct {
	span astSpan
	name string
	args []astNode
}

func (n *astCall) astSpan() astSpan { return n.span }
func (n *astCall) Kind() ASTKind    { return ASTCall }
func (n *astCall) Span() Span       { return publicSpan(n.span) }
func (n *astCall) String() string   { return printAST(n) }
func (n *astCall) Children() []ASTNode {
	return publicChildSlice(n.args)
}

type astUnary struct {
	span  astSpan
	op    astOperator
	rawOp string
	right astNode
}

func (n *astUnary) astSpan() astSpan { return n.span }
func (n *astUnary) Kind() ASTKind    { return ASTUnary }
func (n *astUnary) Span() Span       { return publicSpan(n.span) }
func (n *astUnary) String() string   { return printAST(n) }
func (n *astUnary) Children() []ASTNode {
	return publicChildren(n.right)
}

type astBinary struct {
	span  astSpan
	left  astNode
	op    astOperator
	rawOp string
	right astNode
}

func (n *astBinary) astSpan() astSpan { return n.span }
func (n *astBinary) Kind() ASTKind    { return ASTBinary }
func (n *astBinary) Span() Span       { return publicSpan(n.span) }
func (n *astBinary) String() string   { return printAST(n) }
func (n *astBinary) Children() []ASTNode {
	return publicChildren(n.left, n.right)
}

type astLowerError struct {
	span astSpan
	msg  string
}

func (e *astLowerError) Error() string { return e.msg }

func lowerAST(node astNode) (Rule, error) {
	switch n := node.(type) {
	case *astLiteral:
		r, err := parseValueToken(n.kind, []byte(n.raw))
		if err != nil {
			return nil, &astLowerError{span: n.span, msg: err.Error()}
		}
		return r, nil
	case *astPath:
		if len(n.segments) == 1 && !n.segments[0].bracket && !n.segments[0].isIndex {
			return FieldValue(n.segments[0].key), nil
		}
		segments := append([]pathSegment(nil), n.segments...)
		return &PathValue{segments: segments}, nil
	case *astArray:
		vals := make([]Rule, 0, len(n.vals))
		for _, val := range n.vals {
			r, err := lowerAST(val)
			if err != nil {
				return nil, err
			}
			vals = append(vals, r)
		}
		return newArrayValue(vals), nil
	case *astCall:
		args := make([]Rule, 0, len(n.args))
		for _, arg := range n.args {
			r, err := lowerAST(arg)
			if err != nil {
				return nil, err
			}
			args = append(args, r)
		}
		return newFunctionValue(n.name, args), nil
	case *astUnary:
		right, err := lowerAST(n.right)
		if err != nil {
			return nil, err
		}
		if n.op == astOpNot {
			return &nodeNot{right: right}, nil
		}
	case *astBinary:
		left, err := lowerAST(n.left)
		if err != nil {
			return nil, err
		}
		right, err := lowerAST(n.right)
		if err != nil {
			return nil, err
		}
		switch n.op {
		case astOpAnd:
			return &nodeAnd{left: left, right: right}, nil
		case astOpOr:
			return &nodeOr{left: left, right: right}, nil
		case astOpEQ, astOpNE, astOpContains, astOpGT, astOpGE, astOpLT, astOpLE:
			return &nodeCompare{lv: left, op: tokenKindFromASTOperator(n.op), rv: right}, nil
		case astOpMatches:
			return &nodeMatch{lv: left, rv: right}, nil
		case astOpIn:
			if literalIs[*net.IPNet](right) {
				return &nodeCompare{lv: left, op: op_EQ, rv: right}, nil
			}
			return &nodeIn{lv: left, rv: right}, nil
		}
	}
	return nil, &astLowerError{span: node.astSpan(), msg: "unsupported AST node"}
}

func astLiteralIs(node astNode, kind int) bool {
	lit, ok := node.(*astLiteral)
	return ok && lit.kind == kind
}

func astValidInequalityOperand(node astNode) bool {
	switch n := node.(type) {
	case *astPath, *astCall:
		return true
	case *astLiteral:
		return n.kind == token_INT || n.kind == token_FLOAT
	default:
		return false
	}
}

func printAST(node astNode) string {
	return printASTWithParent(node, 0, false)
}

func printASTWithParent(node astNode, parentPrec int, rightChild bool) string {
	prec := astPrecedence(node)
	var out string

	switch n := node.(type) {
	case *astLiteral:
		out = n.raw
	case *astPath:
		out = (&PathValue{segments: n.segments}).String()
	case *astArray:
		parts := make([]string, 0, len(n.vals))
		for _, val := range n.vals {
			parts = append(parts, printAST(val))
		}
		out = "[" + strings.Join(parts, ", ") + "]"
	case *astCall:
		parts := make([]string, 0, len(n.args))
		for _, arg := range n.args {
			parts = append(parts, printAST(arg))
		}
		out = n.name + "(" + strings.Join(parts, ", ") + ")"
	case *astUnary:
		right := printASTWithParent(n.right, astPrecedence(n), true)
		if _, ok := n.right.(*astBinary); ok {
			right = "(" + printAST(n.right) + ")"
		}
		out = "not " + right
	case *astBinary:
		out = printASTWithParent(n.left, prec, false) + " " + astOperatorString(n.op) + " " + printASTWithParent(n.right, prec, true)
	}

	if prec > 0 && (prec < parentPrec || (rightChild && prec == parentPrec)) {
		return "(" + out + ")"
	}
	return out
}

func astPrecedence(node astNode) int {
	switch n := node.(type) {
	case *astUnary:
		return 4
	case *astBinary:
		switch n.op {
		case astOpOr:
			return 1
		case astOpAnd:
			return 2
		case astOpEQ, astOpNE, astOpGT, astOpGE, astOpLT, astOpLE, astOpContains, astOpMatches, astOpIn:
			return 3
		}
	}
	return 5
}

func astOperatorString(op astOperator) string {
	switch op {
	case astOpAnd:
		return "and"
	case astOpOr:
		return "or"
	case astOpMatches:
		return "=~"
	case astOpIn:
		return "in"
	default:
		return operatorToString(tokenKindFromASTOperator(op))
	}
}
