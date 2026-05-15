package rulekit

import "net"

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

type astPath struct {
	span     astSpan
	segments []pathSegment
}

func (n *astPath) astSpan() astSpan { return n.span }

type astArray struct {
	span astSpan
	vals []astNode
}

func (n *astArray) astSpan() astSpan { return n.span }

type astCall struct {
	span astSpan
	name string
	args []astNode
}

func (n *astCall) astSpan() astSpan { return n.span }

type astUnary struct {
	span  astSpan
	op    astOperator
	rawOp string
	right astNode
}

func (n *astUnary) astSpan() astSpan { return n.span }

type astBinary struct {
	span  astSpan
	left  astNode
	op    astOperator
	rawOp string
	right astNode
}

func (n *astBinary) astSpan() astSpan { return n.span }

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
