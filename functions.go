package rulekit

import (
	"errors"
	"fmt"
	"strings"
)

type FunctionValue struct {
	fn   string
	args *ArrayValue
}

func (f *FunctionValue) Eval(ctx *Ctx) Result {
	// WIP testing function parsing
	fmt.Printf("%s(...)\n", f.fn)
	for _, arg := range f.args.vals {
		fmt.Printf(": %T %s\n", arg, arg.String())
	}
	return Result{
		Error:         errors.New("not implemented"),
		EvaluatedRule: f,
	}
}

func (f *FunctionValue) String() string {
	return f.fn + "(" + f.args.String() + ")"
}

func newFunctionValue(fn string, args *ArrayValue) *FunctionValue {
	args.raw = strings.TrimPrefix(args.raw, "[")
	args.raw = strings.TrimSuffix(args.raw, "]")
	return &FunctionValue{
		fn:   fn,
		args: args,
	}
}
