package rulekit

import (
	"fmt"
	"sort"
	"strings"
)

// Edit replaces one AST node with another AST.
type Edit struct {
	Target      ASTNode
	Replacement *AST
}

// Rewrite applies AST-node replacements while preserving unchanged source text.
func Rewrite(ast *AST, edits []Edit, mode PrintMode) (string, error) {
	if ast == nil || ast.root == nil {
		return "", fmt.Errorf("AST must not be nil")
	}
	if len(edits) == 0 {
		return ast.source, nil
	}

	type rewriteEdit struct {
		span        Span
		replacement string
	}
	rewrites := make([]rewriteEdit, 0, len(edits))
	for _, edit := range edits {
		if edit.Target == nil {
			return "", fmt.Errorf("edit target must not be nil")
		}
		if edit.Replacement == nil {
			return "", fmt.Errorf("edit replacement must not be nil")
		}
		span := edit.Target.Span()
		if span.Start < 0 || span.End < span.Start || span.End > len(ast.source) {
			return "", fmt.Errorf("edit target span is outside source")
		}
		replacement := Format(edit.Replacement, mode)
		rewrites = append(rewrites, rewriteEdit{span: span, replacement: replacement})
	}

	sort.Slice(rewrites, func(i, j int) bool {
		return rewrites[i].span.Start < rewrites[j].span.Start
	})

	var out strings.Builder
	pos := 0
	for _, edit := range rewrites {
		if edit.span.Start < pos {
			return "", fmt.Errorf("edits must not overlap")
		}
		out.WriteString(ast.source[pos:edit.span.Start])
		out.WriteString(edit.replacement)
		pos = edit.span.End
	}
	out.WriteString(ast.source[pos:])
	return out.String(), nil
}
