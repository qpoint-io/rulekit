package rulekit

import "fmt"

type ErrIncomparable struct {
	Field      any
	FieldValue any
	Value      any
	Operator   string
}

func (e *ErrIncomparable) Error() string {
	return fmt.Sprintf("incompatible types: cannot compare field %s [%T] to a %T value using %s", e.Field, e.FieldValue, e.Value, e.Operator)
}

type errIncomparable struct {
	Left     any
	Right    any
	Operator int
}

func (e *errIncomparable) Error() string {
	return fmt.Sprintf("internal error: cannot compare %T to %T using %s", e.Left, e.Right, operatorToString(e.Operator))
}

func (e *errIncomparable) ToPublicError(leftName string, operator int) *ErrIncomparable {
	op := operator
	if e.Operator != 0 {
		op = e.Operator
	}
	return &ErrIncomparable{
		Field:      leftName,
		FieldValue: e.Left,
		Value:      e.Right,
		Operator:   operatorToString(op),
	}
}

func convertIncomparableError(err error, leftName string, operator int) error {
	if err == nil {
		return nil
	}
	if e, ok := err.(*errIncomparable); ok {
		return e.ToPublicError(leftName, operator)
	}
	return err
}
