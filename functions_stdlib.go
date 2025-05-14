package rulekit

import (
	"fmt"
	"strings"
)

var StdlibFuncs = map[string]*Function{
	"starts_with": {
		Args: []FunctionArg{
			{Name: "value"},
			{Name: "prefix"},
		},
		Eval: func(args map[string]any) Result {
			value, err := IndexFuncArg[any](args, "value")
			if err != nil {
				return Result{Error: err}
			}
			prefix, err := IndexFuncArg[any](args, "prefix")
			if err != nil {
				return Result{Error: err}
			}

			return Result{
				Value: strings.HasPrefix(fmt.Sprint(value), fmt.Sprint(prefix)),
			}
		},
	},
}
