package rulekit

import "strings"

var StdlibFuncs = map[string]*Function{
	"starts_with": {
		Args: []FunctionArg{
			{Name: "value"},
			{Name: "prefix"},
		},
		Eval: func(args map[string]any) Result {
			value, err := stringableArg(args, "value")
			if err != nil {
				return Result{Error: err}
			}
			prefix, err := stringableArg(args, "prefix")
			if err != nil {
				return Result{Error: err}
			}
			return Result{Value: strings.HasPrefix(value, prefix)}
		},
	},
}

// stringableArg returns a function argument's text if it is a string or a
// stringable value.
func stringableArg(args map[string]any, name string) (string, error) {
	value, err := IndexFuncArg[any](args, name)
	if err != nil {
		return "", err
	}
	s, ok := stringable(value)
	if !ok {
		return "", &ErrInvalidFunctionArg{Name: name, Expected: "string", Got: diagnosticType(value)}
	}
	return s, nil
}
