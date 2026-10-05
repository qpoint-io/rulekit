// Command unicode_names generates rust/src/regex/unicode_names.rs: the Unicode
// data Go's regexp/syntax parser uses, so the Rust port resolves \p{...}
// classes and (?i) case folding exactly like Go.
//
// Run from this directory:
//
//	go run . > ../../src/regex/unicode_names.rs
//
// The output depends only on the Go toolchain's unicode package. The name
// resolution below is a copy of canonicalName and unicodeTable from
// regexp/syntax/parse.go (Go 1.27.1), which are unexported.
package main

import (
	"bufio"
	"fmt"
	"maps"
	"os"
	"runtime"
	"slices"
	"sort"
	"strings"
	"unicode"
)

var anyTable = &unicode.RangeTable{
	R16: []unicode.Range16{{Lo: 0, Hi: 1<<16 - 1, Stride: 1}},
	R32: []unicode.Range32{{Lo: 1 << 16, Hi: unicode.MaxRune, Stride: 1}},
}

var asciiTable = &unicode.RangeTable{
	R16: []unicode.Range16{{Lo: 0, Hi: 0x7F, Stride: 1}},
}

var asciiFoldTable = &unicode.RangeTable{
	R16: []unicode.Range16{
		{Lo: 0, Hi: 0x7F, Stride: 1},
		{Lo: 0x017F, Hi: 0x017F, Stride: 1},
		{Lo: 0x212A, Hi: 0x212A, Stride: 1},
	},
}

var aliases struct {
	categories map[string]string
	scripts    map[string]string
}

func initAliases() {
	aliases.categories = make(map[string]string)
	aliases.scripts = make(map[string]string)
	for name, actual := range unicode.CategoryAliases {
		aliases.categories[canonicalName(name)] = actual
	}
	for name := range unicode.Scripts {
		aliases.scripts[canonicalName(name)] = name
	}
}

// canonicalName is regexp/syntax.canonicalName.
func canonicalName(name string) string {
	var b []byte
	first := true
	for i := range len(name) {
		c := name[i]
		switch {
		case c == '_' || c == '-' || c == ' ':
			c = ' '
		case first:
			if 'a' <= c && c <= 'z' {
				c -= 'a' - 'A'
			}
			first = false
		default:
			if 'A' <= c && c <= 'Z' {
				c += 'a' - 'A'
			}
		}
		if b == nil {
			if c == name[i] && c != ' ' {
				continue
			}
			b = make([]byte, i, len(name))
			copy(b, name[:i])
		}
		if c == ' ' {
			continue
		}
		b = append(b, c)
	}
	if b == nil {
		return name
	}
	return string(b)
}

// unicodeTable is regexp/syntax.unicodeTable.
func unicodeTable(name string) (tab, fold *unicode.RangeTable, sign int) {
	name = canonicalName(name)
	switch name {
	case "Any":
		return anyTable, anyTable, +1
	case "Assigned":
		return unicode.Cn, unicode.Cn, -1
	case "Ascii":
		return asciiTable, asciiFoldTable, +1
	case "Lc":
		return unicode.Categories["LC"], unicode.FoldCategory["LC"], +1
	}
	if t := unicode.Categories[name]; t != nil {
		return t, unicode.FoldCategory[name], +1
	}
	if t := unicode.Scripts[name]; t != nil {
		return t, unicode.FoldScript[name], +1
	}
	if actual := aliases.categories[name]; actual != "" {
		t := unicode.Categories[actual]
		return t, unicode.FoldCategory[actual], +1
	}
	if actual := aliases.scripts[name]; actual != "" {
		t := unicode.Scripts[actual]
		return t, unicode.FoldScript[actual], +1
	}
	return nil, nil, 0
}

type rng struct{ lo, hi rune }

// ranges returns the code points of t as sorted, merged ranges.
func ranges(t *unicode.RangeTable) []rng {
	var out []rng
	add := func(lo, hi, stride rune) {
		if stride == 1 {
			out = append(out, rng{lo, hi})
			return
		}
		for c := lo; c <= hi; c += stride {
			out = append(out, rng{c, c})
		}
	}
	for _, r := range t.R16 {
		add(rune(r.Lo), rune(r.Hi), rune(r.Stride))
	}
	for _, r := range t.R32 {
		add(rune(r.Lo), rune(r.Hi), rune(r.Stride))
	}
	sort.Slice(out, func(i, j int) bool { return out[i].lo < out[j].lo })
	var merged []rng
	for _, r := range out {
		if n := len(merged); n > 0 && r.lo <= merged[n-1].hi+1 {
			merged[n-1].hi = max(merged[n-1].hi, r.hi)
			continue
		}
		merged = append(merged, r)
	}
	return merged
}

func key(rs []rng) string {
	var b strings.Builder
	for _, r := range rs {
		fmt.Fprintf(&b, "%x-%x,", r.lo, r.hi)
	}
	return b.String()
}

type entry struct {
	name      string
	tab, fold int // indexes into tables; fold < 0 means nil
	negate    bool
}

func main() {
	initAliases()

	candidates := map[string]bool{"Any": true, "Assigned": true, "Ascii": true, "Lc": true}
	for name := range unicode.Categories {
		candidates[canonicalName(name)] = true
	}
	for name := range unicode.Scripts {
		candidates[canonicalName(name)] = true
	}
	for name := range unicode.CategoryAliases {
		candidates[canonicalName(name)] = true
	}

	var tables [][]rng
	tableIndex := map[string]int{}
	intern := func(t *unicode.RangeTable) int {
		rs := ranges(t)
		k := key(rs)
		if i, ok := tableIndex[k]; ok {
			return i
		}
		tableIndex[k] = len(tables)
		tables = append(tables, rs)
		return len(tables) - 1
	}

	var entries []entry
	for _, name := range slices.Sorted(maps.Keys(candidates)) {
		tab, fold, sign := unicodeTable(name)
		if tab == nil {
			fmt.Fprintf(os.Stderr, "candidate %q does not resolve\n", name)
			os.Exit(1)
		}
		e := entry{name: name, tab: intern(tab), fold: -1, negate: sign < 0}
		if fold != nil {
			e.fold = intern(fold)
		}
		entries = append(entries, e)
	}

	var folds []rng // (r, SimpleFold(r)) pairs where they differ
	for r := rune(0); r <= unicode.MaxRune; r++ {
		if f := unicode.SimpleFold(r); f != r {
			folds = append(folds, rng{r, f})
		}
	}

	w := bufio.NewWriter(os.Stdout)
	defer w.Flush()
	fmt.Fprintf(w, `// Code generated by rust/tools/unicode_names; DO NOT EDIT.
//
//   cd rust/tools/unicode_names && go run . > ../../src/regex/unicode_names.rs
//
// Go version: %s. Unicode version: %s.
//
// The Unicode data Go's regexp/syntax uses: every \p{...} name its
// unicodeTable accepts (keyed by canonicalName) with the class table and the
// case-folding table, and unicode.SimpleFold for every rune it changes.

/// A Unicode class name accepted by Go's regexp/syntax.
pub(super) struct UnicodeClass {
    /// The canonical name (Go regexp/syntax canonicalName).
    pub(super) name: &'static str,
    /// The class as sorted, non-overlapping, non-adjacent ranges.
    pub(super) table: &'static [(u32, u32)],
    /// Additional fold-equivalent code points used under (?i); None when Go
    /// has no fold table for the class.
    pub(super) fold: Option<&'static [(u32, u32)]>,
    /// Whether the class is the negation of table (Go's sign < 0).
    pub(super) negate: bool,
}

`, runtime.Version(), unicode.Version)

	fmt.Fprintf(w, "/// Sorted by name; look up with canonicalName(name).\n#[rustfmt::skip]\npub(super) static GO_UNICODE_NAMES: &[UnicodeClass] = &[\n")
	for _, e := range entries {
		fold := "None"
		if e.fold >= 0 {
			fold = fmt.Sprintf("Some(T%d)", e.fold)
		}
		fmt.Fprintf(w, "    UnicodeClass { name: %q, table: T%d, fold: %s, negate: %v },\n", e.name, e.tab, fold, e.negate)
	}
	fmt.Fprintf(w, "];\n\n")

	writeRanges := func(name, doc string, rs []rng) {
		fmt.Fprintf(w, "%s#[rustfmt::skip]\nstatic %s: &[(u32, u32)] = &[", doc, name)
		for i, r := range rs {
			if i%6 == 0 {
				fmt.Fprintf(w, "\n   ")
			}
			fmt.Fprintf(w, " (0x%x, 0x%x),", r.lo, r.hi)
		}
		fmt.Fprintf(w, "\n];\n\n")
	}
	for i, t := range tables {
		writeRanges(fmt.Sprintf("T%d", i), "", t)
	}
	fmt.Fprintf(w, "/// (r, unicode.SimpleFold(r)) for every rune r that SimpleFold changes, sorted by r.\n#[rustfmt::skip]\npub(super) static SIMPLE_FOLD: &[(u32, u32)] = &[")
	for i, r := range folds {
		if i%6 == 0 {
			fmt.Fprintf(w, "\n   ")
		}
		fmt.Fprintf(w, " (0x%x, 0x%x),", r.lo, r.hi)
	}
	fmt.Fprintf(w, "\n];\n")
}
