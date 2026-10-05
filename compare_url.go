package rulekit

import "net/url"

func compareURL(left *url.URL, op int, right any) compareOutcome {
	switch right := right.(type) {
	case *url.URL:
		// url ? url
		return compareStringString(left.String(), op, right.String())
	case string:
		// url ? string
		return compareStringString(left.String(), op, right)
	}
	return incomparable()
}
