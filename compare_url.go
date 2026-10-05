package rulekit

import (
	"net/url"
	"regexp"
)

func compareURL(left *url.URL, op int, right any) compareOutcome {
	switch right := right.(type) {
	case *url.URL:
		// url ? url
		return compareStringString(left.String(), op, right.String())
	case string, *regexp.Regexp, urlQuery:
		// url ? string: compare the URL's text form
		return compareString(left.String(), op, right)
	}
	return incomparable()
}
