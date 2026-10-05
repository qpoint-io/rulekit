package rulekit

import (
	"context"
	"strings"
)

// StdlibFuncs are the built-in functions, available in every rule.
var StdlibFuncs = NewFunctionSet(stdlibStartsWith)

type startsWithArgs struct {
	Value  any
	Prefix any
}

var stdlibStartsWith = Func(FuncSchema{
	Name: "starts_with",
	Doc:  "Reports whether value starts with prefix. Both must be strings or values with a text form (IP address, CIDR, MAC address, URL).",
}, func(_ context.Context, a startsWithArgs) (bool, error) {
	value, err := stringableArg("value", a.Value)
	if err != nil {
		return false, err
	}
	prefix, err := stringableArg("prefix", a.Prefix)
	if err != nil {
		return false, err
	}
	return strings.HasPrefix(value, prefix), nil
})

// stringableArg returns a function argument's text if it is a string or a
// stringable value.
func stringableArg(name string, value any) (string, error) {
	s, ok := stringable(value)
	if !ok {
		return "", &ErrInvalidFunctionArg{Name: name, Expected: "string", Got: diagnosticType(value)}
	}
	return s, nil
}
