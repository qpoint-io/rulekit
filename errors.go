package rulekit

import (
	"errors"
	"fmt"

	"github.com/hashicorp/go-multierror"
	"github.com/qpoint-io/rulekit/set"
)

type ErrMissingFields struct {
	Fields set.Set[string]
}

func (e ErrMissingFields) Error() string {
	return fmt.Sprintf("missing fields: %v", e.Fields.Items())
}

func coalesceErrs(errs ...error) error {
	var (
		multi = &multierror.Error{
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
		// combine all ErrMissingFields errors
		mf = set.NewSet[string]()
	)

	for _, err := range errs {
		if e, ok := err.(*ErrMissingFields); ok {
			mf.Merge(e.Fields)
			continue
		}

		multi = multierror.Append(multi, err)
	}

	if mf.Len() > 0 {
		multi = multierror.Append(multi, &ErrMissingFields{Fields: mf})
	}

	switch len(multi.Errors) {
	case 0:
		return nil
	case 1:
		return multi.Errors[0]
	default:
		return multi
	}
}

var ErrInvalidOperation = errors.New("invalid operation")

type ErrInvalidFunctionArg struct {
	Index    int
	Expected string
	Got      string
}

func (e *ErrInvalidFunctionArg) Error() string {
	return fmt.Sprintf("arg %d: expected %s, got %s", e.Index, e.Expected, e.Got)
}
