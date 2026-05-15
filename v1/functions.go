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
	if fn, ok := StdlibFuncs[f.fn]; ok {
		return f.eval(fn, ctx)
	} else if fn, ok := ctx.Functions[f.fn]; ok {
		return f.eval(fn, ctx)
	} else if macro, ok := ctx.Macros[f.fn]; ok {
		if len(f.args.vals) > 0 {
			return Result{
				Error:         fmt.Errorf("macro %q expects 0 arguments, got %d", f.fn, len(f.args.vals)),
				EvaluatedRule: f,
			}
		}
		return macro.Eval(ctx)
	}

	return Result{
		Error:         fmt.Errorf("unknown function %q", f.fn),
		EvaluatedRule: f,
	}
}

func (f *FunctionValue) eval(fn *Function, ctx *Ctx) Result {
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

func newFunctionValue(fn string, args []Rule) *FunctionValue {
	argsArr := newArrayValue(args)
	argsArr.raw = strings.TrimPrefix(argsArr.raw, "[")
	argsArr.raw = strings.TrimSuffix(argsArr.raw, "]")
	return &FunctionValue{
		fn:   fn,
		args: argsArr,
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
}

func IndexFuncArg[T any](args map[string]any, name string) (T, error) {
	var zeroVal T

	valAny, ok := args[name]
	if !ok {
		return zeroVal, fmt.Errorf("unrecognized argument name %q", name)
	}
	val, ok := valAny.(T)
	if !ok {
		return zeroVal, &ErrInvalidFunctionArg{
			Name:     name,
			Expected: fmt.Sprintf("%T", zeroVal),
			Got:      fmt.Sprintf("%T", valAny),
		}
	}
	return val, nil
}
