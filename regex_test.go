package rulekit

import (
	"regexp"
	"testing"
)

// Every allowed Unicode class name must be one Go's regexp knows.
func TestUnicodeClassesCompile(t *testing.T) {
	if len(unicodeClasses) != 249 {
		t.Errorf("unicodeClasses has %d names, want 249", len(unicodeClasses))
	}
	for key := range unicodeClasses {
		if _, err := regexp.Compile(`\p{` + key + `}`); err != nil {
			t.Errorf("allowed class %q: %v", key, err)
		}
	}
}

func TestCheckRegexDialect(t *testing.T) {
	for _, pattern := range []string{
		`\p{Greek}`, `\p{greek}`, `\p{OLD italic}`, `\pL`, `[\pL\pN]`, `[\pL-]`,
		`(?P<a_1>x)(?<_b>y)`, `(?i)a`, `(?i-s:a)`, `(?:a)*`, `\x{D7FF}`, `\x{E000}`,
		`a{0}`, `a{10}`, `a{0,10}`, `[]-]`, `[^]-]`, `[]a-]`, `(((a)))`,
	} {
		if err := checkRegexDialect(pattern); err != nil {
			t.Errorf("%s: unexpected error: %v", pattern, err)
		}
	}
	for _, pattern := range []string{
		`\p{LC}`, `\p{Cased_Letter}`, `\pC\p{cased letter}`, `\p{Cs}`, `\P{Surrogate}`,
		`[\p{Tai_Yo}]`, `\p{Sidetic}`, `\p{IsGreek}`, `(?)a`, `a(?i)*`, `a(?i)+`,
		`a(?i)?`, `a(?i){2}`, `(?ii)a`, `(?i-i)a`, `(?P<1a>x)`, `(?<a-b>x)`,
		`(?P<n>a)(?P<n>b)`, `(?<m>x(?<m>y))`, `\x{D800}`, `[\x{DFFF}]`,
		`[\pL-\pN]`, `[\PL-b]`, `a{01}`, `a{1,02}`, `a{00}`, `[]-a]`, `[^]-a]`,
	} {
		if err := checkRegexDialect(pattern); err == nil {
			t.Errorf("%s: no error", pattern)
		}
	}
}

func TestRegexGroupDepth(t *testing.T) {
	nest := func(n int, open, inner string) string {
		for range n {
			inner = open + inner + ")"
		}
		return inner
	}
	for _, tc := range []struct {
		pattern string
		ok      bool
	}{
		{nest(50, "(", "a"), true},
		{nest(51, "(", "a"), false},
		{nest(50, "(?:", "a"), true},
		{nest(51, "(?:", "a"), false},
		{"(?i)" + nest(50, "(", "a"), true},
		{nest(49, "(", "(?i)a"), true},
		{nest(50, "(", "(?i)a"), false}, // (?i) counts as an open group
		{nest(50, "(", "a") + nest(50, "(", "a"), true},
		{nest(50, "(", `\(`), true},
		{nest(50, "(", "[(]"), true},
	} {
		if err := checkRegexDialect(tc.pattern); (err == nil) != tc.ok {
			t.Errorf("%s: error %v, want ok %v", tc.pattern, err, tc.ok)
		}
	}
}
