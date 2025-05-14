package rulekit

import (
	"fmt"
	"strings"
)

type FunctionValue struct {
	fn   string
	args *ArrayValue
}

func (f *FunctionValue) Eval(ctx *Ctx) Result {
	fn, ok := StdlibFuncs[f.fn]
	if !ok {
		return Result{
			Error:         fmt.Errorf("unknown function %q", f.fn),
			EvaluatedRule: f,
		}
	}

	if len(fn.Args) != len(f.args.vals) {
		return Result{
			Error:         fmt.Errorf("function %q expects %d arguments, got %d", f.fn, len(fn.Args), len(f.args.vals)),
			EvaluatedRule: f,
		}
	}

	argMap := make(map[string]any, len(f.args.vals))
	for i, arg := range f.args.vals {
		res := arg.Eval(ctx)
		if !res.Ok() {
			return res
		}
		argMap[fn.Args[i].Name] = res.Value
	}
	res := fn.Eval(argMap)
	res.EvaluatedRule = f
	return res
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

func (f *FunctionValue) ValidateStdlibFnArgs() error {
	if stdlibFn, ok := StdlibFuncs[f.fn]; ok {
		if len(stdlibFn.Args) != len(f.args.vals) {
			return fmt.Errorf("function %q expects %d arguments, got %d", f.fn, len(stdlibFn.Args), len(f.args.vals))
		}
	}
	return nil
}

type Function struct {
	// Args is an optional list of arguments that the function expects.
	// If set, rulekit will ensure validity of the arguments and pass them as a named map to the Eval function.
	Args []FunctionArg
	// Eval is the function that will be called with the arguments.
	// EvaluatedRule will be set by Rulekit.
	Eval func(map[string]any) Result
}

type FunctionArg struct {
	Name string
	Type string
}

func IndexFnArg[T any](args map[string]any, idx int, name string) (T, error) {
	var zeroVal T

	valAny, ok := args[name]
	if !ok {
		return zeroVal, fmt.Errorf("unrecognized argument name %q", name)
	}
	val, ok := valAny.(T)
	if !ok {
		return zeroVal, &ErrInvalidFunctionArg{
			Index:    idx,
			Expected: fmt.Sprintf("%T", valAny),
			Got:      fmt.Sprintf("%T", valAny),
		}
	}
	return val, nil
}
