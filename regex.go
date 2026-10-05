package rulekit

import (
	"fmt"
	"strings"
)

// checkRegexDialect rejects regex forms that regex engines interpret
// differently, so a pattern means the same thing in every rulekit
// implementation. Patterns are otherwise compiled with RE2 syntax.
func checkRegexDialect(pattern string) error {
	inClass := false
	classStart := 0
	for i := 0; i < len(pattern); i++ {
		c := pattern[i]
		if c == '\\' {
			if i+1 >= len(pattern) {
				return nil // the compiler reports the trailing backslash
			}
			next := pattern[i+1]
			switch {
			case next == '<' || next == '>':
				return fmt.Errorf(`\%c is not supported; use \b for word boundaries`, next)
			case next == 'Q' || next == 'E':
				return fmt.Errorf(`\Q...\E is not supported; escape each character instead`)
			case next >= '0' && next <= '9':
				return fmt.Errorf(`\%c is not supported; use \x{...} for character codes`, next)
			case (next == 'b' || next == 'B') && i+2 < len(pattern) && pattern[i+2] == '{':
				return fmt.Errorf(`\%c{...} is not supported`, next)
			case (next == 'p' || next == 'P') && strings.HasPrefix(pattern[i+2:], "{^"):
				return fmt.Errorf(`\%c{^...} is not supported; use \P{...} to negate a Unicode class`, next)
			}
			end := i + 1
			if (next == 'p' || next == 'P' || next == 'x') && i+2 < len(pattern) && pattern[i+2] == '{' {
				if close := strings.IndexByte(pattern[i+2:], '}'); close >= 0 {
					end = i + 2 + close
				}
			}
			if inClass && strings.IndexByte("dDsSwWpP", next) >= 0 &&
				end+2 < len(pattern) && pattern[end+1] == '-' && pattern[end+2] != ']' {
				return fmt.Errorf(`\%c cannot start a range in a character class; escape the dash as \-`, next)
			}
			i = end
			continue
		}

		if inClass {
			switch {
			case c == ']' && i > classStart:
				inClass = false
			case c == '[':
				if strings.HasPrefix(pattern[i:], "[:") {
					if close := strings.Index(pattern[i+2:], ":]"); close >= 0 {
						i += 2 + close + 1
						continue
					}
				}
				return fmt.Errorf(`nested character classes are not supported; escape [ as \[`)
			case (c == '&' || c == '-' || c == '~') && i+1 < len(pattern) && pattern[i+1] == c:
				return fmt.Errorf(`%c%c in a character class is not supported; escape it as \%c\%c`, c, c, c, c)
			}
			continue
		}

		switch {
		case c == '[':
			inClass = true
			classStart = i + 1
			if classStart < len(pattern) && pattern[classStart] == '^' {
				classStart++
			}
		case c == '{' && i+1 < len(pattern) && pattern[i+1] == ',':
			return fmt.Errorf(`{,n} is not supported; use {0,n}`)
		case c == '{' && !isRepetition(pattern[i:]):
			return fmt.Errorf(`{ must start a repetition such as {2} or {1,3}; escape a literal brace as \{`)
		}
	}
	return nil
}

// isRepetition reports whether s starts with a counted repetition: {n},
// {n,}, or {n,m}.
func isRepetition(s string) bool {
	i := skipDigits(s, 1)
	if i == 1 {
		return false
	}
	if i < len(s) && s[i] == ',' {
		i = skipDigits(s, i+1)
	}
	return i < len(s) && s[i] == '}'
}
