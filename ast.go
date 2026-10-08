package rulekit

import (
	"net"
	"strings"
)

func spanFromToken(tok token) Span {
	return Span{Start: tok.start, End: tok.end}
}

func joinSpan(left, right Span) Span {
	if left.Start == 0 && left.End == 0 {
		return right
	}
	if right.Start == 0 && right.End == 0 {
		return left
	}
	return Span{Start: left.Start, End: right.End}
}

func astOperatorFromToken(kind int) Operator {
	switch kind {
	case op_NOT:
		return OperatorNot
	case op_AND:
		return OperatorAnd
	case op_OR:
		return OperatorOr
	case op_EQ:
		return OperatorEQ
	case op_NE:
		return OperatorNE
	case op_GT:
		return OperatorGT
	case op_GE:
		return OperatorGE
	case op_LT:
		return OperatorLT
	case op_LE:
		return OperatorLE
	case op_CONTAINS:
		return OperatorContains
	case op_MATCHES:
		return OperatorMatches
	case op_IN:
		return OperatorIn
	default:
		return OperatorUnknown
	}
}

func tokenKindFromASTOperator(op Operator) int {
	switch op {
	case OperatorNot:
		return op_NOT
	case OperatorAnd:
		return op_AND
	case OperatorOr:
		return op_OR
	case OperatorEQ:
		return op_EQ
	case OperatorNE:
		return op_NE
	case OperatorGT:
		return op_GT
	case OperatorGE:
		return op_GE
	case OperatorLT:
		return op_LT
	case OperatorLE:
		return op_LE
	case OperatorContains:
		return op_CONTAINS
	case OperatorMatches:
		return op_MATCHES
	case OperatorIn:
		return op_IN
	default:
		return 0
	}
}

type astLiteral struct {
	span Span
	kind int
	raw  string
}

func (n *astLiteral) Kind() ASTKind  { return ASTLiteral }
func (n *astLiteral) Span() Span     { return n.span }
func (n *astLiteral) String() string { return printAST(n) }
func (n *astLiteral) Children() []ASTNode {
	return nil
}

type astPath struct {
	span     Span
	segments []pathSegment
}

func (n *astPath) Kind() ASTKind  { return ASTPath }
func (n *astPath) Span() Span     { return n.span }
func (n *astPath) String() string { return printAST(n) }
func (n *astPath) Children() []ASTNode {
	return nil
}

type astArray struct {
	span Span
	vals []ASTNode
}

func (n *astArray) Kind() ASTKind  { return ASTArray }
func (n *astArray) Span() Span     { return n.span }
func (n *astArray) String() string { return printAST(n) }
func (n *astArray) Children() []ASTNode {
	return publicChildSlice(n.vals)
}

type astCall struct {
	span Span
	name string
	args []ASTNode
}

func (n *astCall) Kind() ASTKind  { return ASTCall }
func (n *astCall) Span() Span     { return n.span }
func (n *astCall) String() string { return printAST(n) }
func (n *astCall) Children() []ASTNode {
	return publicChildSlice(n.args)
}

type astUnary struct {
	span  Span
	op    Operator
	rawOp string
	right ASTNode
}

func (n *astUnary) Kind() ASTKind  { return ASTUnary }
func (n *astUnary) Span() Span     { return n.span }
func (n *astUnary) String() string { return printAST(n) }
func (n *astUnary) Children() []ASTNode {
	return publicChildren(n.right)
}

type astBinary struct {
	span  Span
	left  ASTNode
	op    Operator
	rawOp string
	right ASTNode
	// negated marks `not contains`, `not matches`, and `not in`: the base
	// operator with a negation applied on top.
	negated bool
}

func (n *astBinary) Kind() ASTKind  { return ASTBinary }
func (n *astBinary) Span() Span     { return n.span }
func (n *astBinary) String() string { return printAST(n) }
func (n *astBinary) Children() []ASTNode {
	return publicChildren(n.left, n.right)
}

type astLowerError struct {
	span Span
	msg  string
}

func (e *astLowerError) Error() string { return e.msg }

func lowerAST(node ASTNode) (Rule, error) {
	switch n := node.(type) {
	case *astLiteral:
		r, err := parseValueToken(n.kind, n.raw)
		if err != nil {
			return nil, &astLowerError{span: n.span, msg: err.Error()}
		}
		return withTrace(n, r), nil
	case *astPath:
		if len(n.segments) == 1 && !n.segments[0].bracket && !n.segments[0].isIndex {
			return withTrace(n, FieldValue(n.segments[0].key)), nil
		}
		segments := append([]pathSegment(nil), n.segments...)
		return withTrace(n, &PathValue{segments: segments}), nil
	case *astArray:
		vals := make([]Rule, 0, len(n.vals))
		for _, val := range n.vals {
			r, err := lowerAST(val)
			if err != nil {
				return nil, err
			}
			vals = append(vals, r)
		}
		return withTrace(n, newArrayLiteral(vals)), nil
	case *astCall:
		args := make([]Rule, 0, len(n.args))
		for _, arg := range n.args {
			r, err := lowerAST(arg)
			if err != nil {
				return nil, err
			}
			args = append(args, r)
		}
		return withTrace(n, newFunctionValue(n.name, args)), nil
	case *astUnary:
		right, err := lowerAST(n.right)
		if err != nil {
			return nil, err
		}
		if n.op == OperatorNot {
			return withTrace(n, &nodeNot{right: right}), nil
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
		op := n.op
		var r Rule
		switch op {
		case OperatorAnd:
			r = &nodeAnd{left: left, right: right}
		case OperatorOr:
			r = &nodeOr{left: left, right: right}
		case OperatorEQ, OperatorNE, OperatorContains, OperatorGT, OperatorGE, OperatorLT, OperatorLE:
			r = &nodeCompare{lv: left, op: tokenKindFromASTOperator(op), rv: right}
		case OperatorMatches:
			r = &nodeMatch{lv: left, rv: right}
		case OperatorIn:
			if literalIs[*net.IPNet](right) {
				r = &nodeCompare{lv: left, op: op_EQ, rv: right}
			} else {
				r = &nodeIn{lv: left, rv: right}
			}
		}
		if r != nil {
			if n.negated {
				r = &nodeNot{right: r}
			}
			return withTrace(n, r), nil
		}
	}
	return nil, &astLowerError{span: node.Span(), msg: "unsupported AST node"}
}

func astLiteralIs(node ASTNode, kind int) bool {
	lit, ok := node.(*astLiteral)
	return ok && lit.kind == kind
}

func astValidInequalityOperand(node ASTNode) bool {
	switch n := node.(type) {
	case *astPath, *astCall:
		return true
	case *astLiteral:
		return n.kind == token_INT || n.kind == token_FLOAT
	default:
		return false
	}
}

func printAST(node ASTNode) string {
	return printASTWithParent(node, 0, false)
}

func printASTWithParent(node ASTNode, parentPrec int, rightChild bool) string {
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
		// not binds looser than comparisons, so it needs parentheses as a
		// comparison operand.
		if parentPrec >= astPrecedence(&astBinary{op: OperatorEQ}) {
			return "(" + out + ")"
		}
	case *astBinary:
		operator := astOperatorString(n.op)
		if n.negated {
			operator = "not " + operator
		}
		out = printASTWithParent(n.left, prec, false) + " " + operator + " " + printASTWithParent(n.right, prec, true)
	}

	if prec > 0 && (prec < parentPrec || (rightChild && prec == parentPrec)) {
		return "(" + out + ")"
	}
	return out
}

func astPrecedence(node ASTNode) int {
	switch n := node.(type) {
	case *astUnary:
		return 4
	case *astBinary:
		switch n.op {
		case OperatorOr:
			return 1
		case OperatorAnd:
			return 2
		case OperatorEQ, OperatorNE, OperatorGT, OperatorGE, OperatorLT, OperatorLE, OperatorContains, OperatorMatches, OperatorIn:
			return 3
		}
	}
	return 5
}

func astOperatorString(op Operator) string {
	switch op {
	case OperatorAnd:
		return "and"
	case OperatorOr:
		return "or"
	case OperatorMatches:
		return "=~"
	case OperatorIn:
		return "in"
	default:
		return operatorToString(tokenKindFromASTOperator(op))
	}
}
