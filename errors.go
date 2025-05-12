package rulekit

import (
	"fmt"
	"slices"

	"github.com/qpoint-io/rulekit/set"
)

type ErrMissingFields struct {
	Fields set.Set[string]
}

func (e ErrMissingFields) Error() string {
	return fmt.Sprintf("missing fields: %v", e.Fields)
}

func coalesceErrs(errs ...error) []error {
	var (
		// combine all ErrMissingFields errors
		mf ErrMissingFields
	)
	errs = slices.DeleteFunc(errs, func(err error) bool {
		if e, ok := err.(*ErrMissingFields); ok {
			mf.Fields = set.Union(mf.Fields, e.Fields)
			return true
		}
		return false
	})

	return append(errs, &mf)
}
