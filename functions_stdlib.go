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

	// index(container, key)
	//
	// index is used to index into a map via a string key or an array via an integer index.
	//
	// e.g. index(map, "key")
	//      map: map[string]any{"key": "value"} -> "value"
	//
	//      index(["first", "second"], 1) -> "second"
	"index": {
		Args: []FunctionArg{
			{Name: "container"},
			{Name: "key"},
		},
		Eval: func(args map[string]any) Result {
			container, err := IndexFuncArg[any](args, "container")
			if err != nil {
				return Result{Error: err}
			}

			switch c := container.(type) {
			case KV:
				key, err := IndexFuncArg[string](args, "key")
				if err != nil {
					return Result{Error: err}
				}

				val, ok := IndexKV(c, key)
				if !ok {
					return Result{Error: fmt.Errorf("key %q not found", key)}
				}

				return Result{Value: val}

			case []any:
				key, err := IndexFuncArg[int64](args, "key")
				if err != nil {
					return Result{Error: err}
				}

				if key < 0 || int(key) >= len(c) {
					return Result{Error: fmt.Errorf("index %d out of bounds", key)}
				}

				return Result{
					Value: c[key],
				}
			}

			return Result{Error: fmt.Errorf("container must be a map or array")}
		},
	},
}
