package rulekit

import (
	"fmt"
	"strconv"
	"strings"
)

// maxGroupDepth is the most groups that may be open at once. A flag group
// such as (?i) counts as an open group.
const maxGroupDepth = 50

// checkRegexDialect rejects regex forms that regex engines interpret
// differently, so a pattern means the same thing in every rulekit
// implementation. Patterns are otherwise compiled with RE2 syntax.
func checkRegexDialect(pattern string) error {
	inClass := false
	classStart := 0
	depth := 0
	names := map[string]bool{}
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
			switch {
			case (next == 'p' || next == 'P' || next == 'x') && i+2 < len(pattern) && pattern[i+2] == '{':
				if close := strings.IndexByte(pattern[i+2:], '}'); close >= 0 {
					end = i + 2 + close
				}
			case (next == 'p' || next == 'P') && i+2 < len(pattern):
				end = i + 2 // a one-letter class name, as in \pL
			}
			if err := checkEscape(pattern[i : end+1]); err != nil {
				return err
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
			if body := pattern[classStart:]; strings.HasPrefix(body, "]-") && !strings.HasPrefix(body[2:], "]") {
				return fmt.Errorf(`a character class cannot start with a range from ]; escape it as \]`)
			}
		case c == '(':
			if depth == maxGroupDepth {
				return fmt.Errorf("groups cannot nest more than %d deep", maxGroupDepth)
			}
			end, flagsOnly, err := checkGroupStart(pattern, i, names)
			if err != nil {
				return err
			}
			if !flagsOnly {
				depth++
			} else if rest := pattern[end+1:]; rest != "" &&
				(strings.IndexByte("*+?", rest[0]) >= 0 || rest[0] == '{' && isRepetition(rest)) {
				return fmt.Errorf(`%s cannot be repeated; repeat a group such as (?:...) instead`, pattern[i:end+1])
			}
			i = end
		case c == ')' && depth > 0:
			depth--
		case c == '{' && i+1 < len(pattern) && pattern[i+1] == ',':
			return fmt.Errorf(`{,n} is not supported; use {0,n}`)
		case c == '{' && !isRepetition(pattern[i:]):
			return fmt.Errorf(`{ must start a repetition such as {2} or {1,3}; escape a literal brace as \{`)
		case c == '{' && hasLeadingZero(pattern[i:]):
			return fmt.Errorf(`repetition counts cannot have leading zeros, as in {01}`)
		}
	}
	return nil
}

// checkEscape checks the class name of a \p or \P escape and the code point
// of a \x{...} escape. esc is the whole escape, such as \pL or \p{Greek}.
func checkEscape(esc string) error {
	if len(esc) < 3 {
		return nil
	}
	switch esc[1] {
	case 'p', 'P':
		name := strings.TrimSuffix(strings.TrimPrefix(esc[2:], "{"), "}")
		if !unicodeClasses[unicodeClassKey(name)] {
			return fmt.Errorf("%s is not a supported Unicode class", esc)
		}
	case 'x':
		if hex, ok := strings.CutPrefix(esc, `\x{`); ok {
			r, err := strconv.ParseUint(strings.TrimSuffix(hex, "}"), 16, 32)
			if err == nil && r >= 0xD800 && r <= 0xDFFF {
				return fmt.Errorf("%s is a surrogate code point, which is not a character", esc)
			}
		}
	}
	return nil
}

// checkGroupStart checks the group opened at pattern[i] and returns the index
// of the last byte of its opener: "(", "(?P<name>", "(?<name>", "(?flags:",
// or "(?flags)". flagsOnly reports the last, which closes right away.
func checkGroupStart(pattern string, i int, names map[string]bool) (end int, flagsOnly bool, err error) {
	rest := pattern[i+1:]
	if !strings.HasPrefix(rest, "?") {
		return i, false, nil
	}
	if strings.HasPrefix(rest, "?P<") || strings.HasPrefix(rest, "?<") {
		start := i + 1 + strings.IndexByte(rest, '<') + 1
		close := strings.IndexByte(pattern[start:], '>')
		if close < 0 {
			return i, false, nil // the compiler reports the unterminated name
		}
		name := pattern[start : start+close]
		if !isCaptureName(name) {
			return 0, false, fmt.Errorf("invalid capture group name %q; use letters, digits, and _, not starting with a digit", name)
		}
		if names[name] {
			return 0, false, fmt.Errorf("duplicate capture group name %q", name)
		}
		names[name] = true
		return start + close, false, nil
	}
	j := i + 2
	for j < len(pattern) && (pattern[j] == '-' || isASCIILetter(pattern[j])) {
		j++
	}
	if j == len(pattern) || pattern[j] != ')' && pattern[j] != ':' {
		return i, false, nil // the compiler reports the invalid group
	}
	flags := pattern[i+2 : j]
	if flags == "" && pattern[j] == ')' {
		return 0, false, fmt.Errorf("(?) is not supported; remove it")
	}
	for k := range len(flags) {
		if flags[k] != '-' && strings.IndexByte(flags[k+1:], flags[k]) >= 0 {
			return 0, false, fmt.Errorf("duplicate flag %c in %s", flags[k], pattern[i:j+1])
		}
	}
	return j, pattern[j] == ')', nil
}

// isCaptureName reports whether name matches [A-Za-z_][A-Za-z0-9_]*.
func isCaptureName(name string) bool {
	if name == "" || name[0] >= '0' && name[0] <= '9' {
		return false
	}
	for i := range len(name) {
		c := name[i]
		if c != '_' && !isASCIILetter(c) && (c < '0' || c > '9') {
			return false
		}
	}
	return true
}

func isASCIILetter(c byte) bool {
	return 'a' <= c && c <= 'z' || 'A' <= c && c <= 'Z'
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

// hasLeadingZero reports whether the counted repetition at the start of s
// has a count with a leading zero, such as {01}. A lone 0 is fine.
func hasLeadingZero(s string) bool {
	counts, _, _ := strings.Cut(s[1:], "}")
	for n := range strings.SplitSeq(counts, ",") {
		if len(n) > 1 && n[0] == '0' {
			return true
		}
	}
	return false
}

// unicodeClassKey spells a Unicode class name the way Go looks it up:
// without spaces, underscores, and hyphens, with ASCII letters in lowercase.
func unicodeClassKey(name string) string {
	var b strings.Builder
	for i := range len(name) {
		switch c := name[i]; {
		case c == ' ' || c == '_' || c == '-':
		case 'A' <= c && c <= 'Z':
			b.WriteByte(c + 'a' - 'A')
		default:
			b.WriteByte(c)
		}
	}
	return b.String()
}

// unicodeClasses holds the keys (see unicodeClassKey) of the Unicode class
// names \p and \P accept: Go's general categories, their long names, and
// Go's scripts, plus Any, ASCII, and Assigned. Left out, because other
// rulekit implementations cannot match them the same way: LC (Cased_Letter;
// Go does not case fold it under (?i)), Cs (Surrogate), and the Unicode 17
// scripts Beria_Erfe, Sidetic, Tai_Yo, and Tolong_Siki.
var unicodeClasses = func() map[string]bool {
	names := strings.Fields(`
		C Cc Cf Cn Co L Ll Lm Lo Lt Lu M Mc Me Mn N Nd Nl No P Pc Pd Pe Pf Pi
		Po Ps S Sc Sk Sm So Z Zl Zp Zs

		Close_Punctuation Combining_Mark Connector_Punctuation Control
		Currency_Symbol Dash_Punctuation Decimal_Number Enclosing_Mark
		Final_Punctuation Format Initial_Punctuation Letter Letter_Number
		Line_Separator Lowercase_Letter Mark Math_Symbol Modifier_Letter
		Modifier_Symbol Nonspacing_Mark Number Open_Punctuation Other
		Other_Letter Other_Number Other_Punctuation Other_Symbol
		Paragraph_Separator Private_Use Punctuation Separator Space_Separator
		Spacing_Mark Symbol Titlecase_Letter Unassigned Uppercase_Letter cntrl
		digit punct

		Any ASCII Assigned

		Adlam Ahom Anatolian_Hieroglyphs Arabic Armenian Avestan Balinese Bamum
		Bassa_Vah Batak Bengali Bhaiksuki Bopomofo Brahmi Braille Buginese Buhid
		Canadian_Aboriginal Carian Caucasian_Albanian Chakma Cham Cherokee
		Chorasmian Common Coptic Cuneiform Cypriot Cypro_Minoan Cyrillic Deseret
		Devanagari Dives_Akuru Dogra Duployan Egyptian_Hieroglyphs Elbasan
		Elymaic Ethiopic Garay Georgian Glagolitic Gothic Grantha Greek Gujarati
		Gunjala_Gondi Gurmukhi Gurung_Khema Han Hangul Hanifi_Rohingya Hanunoo
		Hatran Hebrew Hiragana Imperial_Aramaic Inherited Inscriptional_Pahlavi
		Inscriptional_Parthian Javanese Kaithi Kannada Katakana Kawi Kayah_Li
		Kharoshthi Khitan_Small_Script Khmer Khojki Khudawadi Kirat_Rai Lao
		Latin Lepcha Limbu Linear_A Linear_B Lisu Lycian Lydian Mahajani Makasar
		Malayalam Mandaic Manichaean Marchen Masaram_Gondi Medefaidrin
		Meetei_Mayek Mende_Kikakui Meroitic_Cursive Meroitic_Hieroglyphs Miao
		Modi Mongolian Mro Multani Myanmar Nabataean Nag_Mundari Nandinagari
		New_Tai_Lue Newa Nko Nushu Nyiakeng_Puachue_Hmong Ogham Ol_Chiki Ol_Onal
		Old_Hungarian Old_Italic Old_North_Arabian Old_Permic Old_Persian
		Old_Sogdian Old_South_Arabian Old_Turkic Old_Uyghur Oriya Osage Osmanya
		Pahawh_Hmong Palmyrene Pau_Cin_Hau Phags_Pa Phoenician Psalter_Pahlavi
		Rejang Runic Samaritan Saurashtra Sharada Shavian Siddham SignWriting
		Sinhala Sogdian Sora_Sompeng Soyombo Sundanese Sunuwar Syloti_Nagri
		Syriac Tagalog Tagbanwa Tai_Le Tai_Tham Tai_Viet Takri Tamil Tangsa
		Tangut Telugu Thaana Thai Tibetan Tifinagh Tirhuta Todhri Toto
		Tulu_Tigalari Ugaritic Vai Vithkuqi Wancho Warang_Citi Yezidi Yi
		Zanabazar_Square
	`)
	classes := make(map[string]bool, len(names))
	for _, name := range names {
		classes[unicodeClassKey(name)] = true
	}
	return classes
}()
