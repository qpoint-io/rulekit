package rulekit

import (
	"net/url"
	"regexp"
)

func compareURL(left *url.URL, op int, right any) compareOutcome {
	switch right := right.(type) {
	case *url.URL:
		// url ? url
		return compareStringString(urlText(left), op, urlText(right))
	case string, *regexp.Regexp, urlQuery:
		// url ? string: compare the URL's text form
		return compareString(urlText(left), op, right)
	}
	return incomparable()
}
