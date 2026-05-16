package rulekit

import (
	"errors"
	"fmt"

	"github.com/hashicorp/go-multierror"
)

func coalesceErrs(errs ...error) error {
	var out []error

	for _, err := range errs {
		if err == nil {
			continue
		}
		out = append(out, err)
	}

	switch len(out) {
	case 0:
		return nil
	case 1:
		return out[0]
	default:
		multi := &multierror.Error{
			Errors: out,
			ErrorFormat: func(errs []error) string {
				switch len(errs) {
				case 0:
					return ""
				case 1:
					return errs[0].Error()
				default:
					return multierror.ListFormatFunc(errs)
				}
			},
		}
		return multi
	}
}

func coalesceMissingFields(left, right []string) []string {
	return unionUnique(left, right)
}

var ErrInvalidOperation = errors.New("invalid operation")

type ErrInvalidFunctionArg struct {
	Name     string
	Expected string
	Got      string
}

func (e *ErrInvalidFunctionArg) Error() string {
	return fmt.Sprintf("arg %s: expected %s, got %s", e.Name, e.Expected, e.Got)
}
