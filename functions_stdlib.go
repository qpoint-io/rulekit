package rulekit

import (
	"context"
	"net"
	"strings"
)

// StdlibFuncs are the built-in functions, available in every rule.
var StdlibFuncs = NewFunctionSet(stdlibStartsWith, stdlibCoalesce, stdlibCompact)

type startsWithArgs struct {
	Value  any
	Prefix any
}

var stdlibStartsWith = Func(FuncSchema{
	Name: "starts_with",
	Doc:  "Reports whether value starts with prefix, or with any prefix in a list of prefixes. value and each prefix must be strings or values with a text form (IP address, CIDR, MAC address, URL). A list is checked in order and stops at the first match; an empty list does not match.",
}, func(_ context.Context, a startsWithArgs) (bool, error) {
	value, err := stringableArg("value", a.Value)
	if err != nil {
		return false, err
	}
	switch prefixes := a.Prefix.(type) {
	case []any:
		return hasAnyPrefix(value, prefixes)
	case []string:
		for _, prefix := range prefixes {
			if strings.HasPrefix(value, prefix) {
				return true, nil
			}
		}
		return false, nil
	case []net.IP:
		return hasAnyPrefix(value, prefixes)
	case []int:
		return hasAnyPrefix(value, prefixes)
	case []int64:
		return hasAnyPrefix(value, prefixes)
	case []uint:
		return hasAnyPrefix(value, prefixes)
	case []uint64:
		return hasAnyPrefix(value, prefixes)
	case []float32:
		return hasAnyPrefix(value, prefixes)
	case []float64:
		return hasAnyPrefix(value, prefixes)
	}
	prefix, err := stringableArg("prefix", a.Prefix)
	if err != nil {
		return false, err
	}
	return strings.HasPrefix(value, prefix), nil
})

// hasAnyPrefix reports whether value starts with one of prefixes, checked in
// order up to the first match. A prefix without a text form is an error.
func hasAnyPrefix[T any](value string, prefixes []T) (bool, error) {
	for _, p := range prefixes {
		prefix, err := stringableArg("prefix", p)
		if err != nil {
			return false, err
		}
		if strings.HasPrefix(value, prefix) {
			return true, nil
		}
	}
	return false, nil
}

// stringableArg returns a function argument's text if it is a string or a
// stringable value.
func stringableArg(name string, value any) (string, error) {
	s, ok := stringable(value)
	if !ok {
		return "", &ErrInvalidFunctionArg{Name: name, Expected: "string", Got: diagnosticType(value)}
	}
	return s, nil
}

type coalesceArgs struct {
	Value  any
	Values Rest
}

// FunctionValue.Eval evaluates coalesce itself, argument by argument and
// stopping at the first present one, which a strict call cannot do; this
// handler is the same choice over arguments that are all evaluated.
var stdlibCoalesce = Func(FuncSchema{
	Name: "coalesce",
	Doc:  "Returns the first argument that is present: not missing from the input and not null. Same as value ?? values... Arguments are evaluated in order and later ones are skipped once one is present; an error stops the search and is the result. If no argument is present, the result is the last argument's (missing, or null). A falsy value such as false, 0, or \"\" is present.",
}, func(_ context.Context, a coalesceArgs) (any, error) {
	if a.Value != nil {
		return a.Value, nil
	}
	for _, v := range a.Values {
		if v != nil {
			return v, nil
		}
	}
	return nil, nil
})

type compactArgs struct {
	List []any
}

// FunctionValue.Eval evaluates an array literal argument element by element,
// dropping the missing ones, before calling the handler with the rest.
var stdlibCompact = Func(FuncSchema{
	Name: "compact",
	Doc:  "Returns list without its null elements. In an array literal, such as compact([a, b]), elements missing from the input are dropped too, so the result is always complete; an element error is the result. Only top-level elements are dropped: false, 0, \"\", and [] are kept. A list that is missing from the input makes the result unknown, and an argument that is not a list is an error.",
}, func(_ context.Context, a compactArgs) ([]any, error) {
	out := make([]any, 0, len(a.List))
	for _, v := range a.List {
		if v != nil {
			out = append(out, v)
		}
	}
	return out, nil
})
