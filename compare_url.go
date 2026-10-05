package rulekit

import (
	"net/url"
	"regexp"
)

// compareURL compares a URL by its text form: with another URL, or with a
// string, regex, or query.
func compareURL(left string, op int, right any) compareOutcome {
	switch right := right.(type) {
	case *url.URL:
		return compareStringString(left, op, urlText(right))
	case URL:
		return compareStringString(left, op, right.text)
	case string, *regexp.Regexp, urlQuery:
		return compareString(left, op, right)
	}
	return incomparable()
}
