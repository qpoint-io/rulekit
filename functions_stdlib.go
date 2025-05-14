package rulekit

import (
	"strings"
)

var StdlibFuncs = map[string]*Function{
	"starts_with": {
		Name: "starts_with",
		Args: []FunctionArg{
			{Name: "str"},
			{Name: "prefix"},
		},
		Eval: func(args map[string]any) Result {
			str, err := IndexFnArg[string](args, 0, "str")
			if err != nil {
				return Result{Error: err}
			}
			prefix, err := IndexFnArg[string](args, 1, "prefix")
			if err != nil {
				return Result{Error: err}
			}

			return Result{Value: strings.HasPrefix(str, prefix)}
		},
	},
}
