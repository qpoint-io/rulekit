package rulekit

import (
	"context"
	"net"
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
