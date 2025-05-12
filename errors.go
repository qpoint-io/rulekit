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
	return fmt.Sprintf("missing fields: %v", e.Fields)
}

func coalesceErrs(errs ...error) error {
	var (
		errors = &multierror.Error{
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

		errors = multierror.Append(errors, err)
	}

	if mf.Len() > 0 {
		errors = multierror.Append(errors, &ErrMissingFields{Fields: mf})
	}

	return errors.ErrorOrNil()
}

var ErrInvalidOperation = errors.New("invalid operation")
