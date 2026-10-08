package rulekit

import (
	"testing"

	"github.com/stretchr/testify/require"
)

func TestPublicASTAPI(t *testing.T) {
	ast, err := ParseAST(`request.headers["user-agent"] == "curl"`)
	require.NoError(t, err)
	require.Equal(t, `request.headers["user-agent"] == "curl"`, ast.String())
	require.Equal(t, `request.headers["user-agent"] == "curl"`, ast.Source())

	root := ast.Root()
	require.Equal(t, ASTBinary, root.Kind())
	require.Equal(t, OperatorEQ, NodeOperator(root))
	require.Equal(t, "==", NodeRawOperator(root))
	require.Len(t, root.Children(), 2)

	segments, ok := NodePath(root.Children()[0])
	require.True(t, ok)
	require.Equal(t, []PathSegment{
		{Key: "request"},
		{Key: "headers"},
		{Key: "user-agent", Bracket: true},
	}, segments)

	rule, err := Compile(ast)
	require.NoError(t, err)
	require.Equal(t, `request.headers["user-agent"] == "curl"`, rule.String())
}

func TestASTTokensIncludeTrivia(t *testing.T) {
	ast, err := ParseAST("field == 1 -- explain\n and other == 2")
	require.NoError(t, err)

	tokens := ast.Tokens()
	require.Len(t, tokens, 8)
	require.Equal(t, "field", tokens[0].Raw)
	require.Equal(t, " ", tokens[0].TrailingTrivia)
	require.Equal(t, " ", tokens[1].LeadingTrivia)
	require.Equal(t, " -- explain\n ", tokens[2].TrailingTrivia)
	require.Equal(t, " -- explain\n ", tokens[3].LeadingTrivia)
	require.Equal(t, Span{Start: 0, End: 5}, ast.Root().Children()[0].Children()[0].Span())
}

func TestRewritePreservesUnchangedSource(t *testing.T) {
	ast, err := ParseAST("field == 1 -- keep\n and other == 2")
	require.NoError(t, err)

	replacement, err := ParseAST(`field == 3`)
	require.NoError(t, err)

	rewritten, err := Rewrite(ast, []Edit{{
		Target:      ast.Root().Children()[0],
		Replacement: replacement,
	}}, Compact())
	require.NoError(t, err)
	require.Equal(t, "field == 3 -- keep\n and other == 2", rewritten)

	roundTrip, err := ParseAST(rewritten)
	require.NoError(t, err)
	require.Equal(t, `field == 3 and other == 2`, roundTrip.String())
}

func TestASTReturnsDefensiveCopies(t *testing.T) {
	for _, expr := range []string{`not field`, `field == 1`, `[field, 1]`, `starts_with(field, "a")`} {
		t.Run(expr, func(t *testing.T) {
			ast, err := ParseAST(expr)
			require.NoError(t, err)
			children := ast.Root().Children()
			original := children[0]
			children[0] = nil
			require.Same(t, original, ast.Root().Children()[0])

			tokens := ast.Tokens()
			originalToken := tokens[0]
			tokens[0] = Token{}
			require.Equal(t, originalToken, ast.Tokens()[0])
		})
	}

	ast, err := ParseAST(`request.headers["user-agent"]`)
	require.NoError(t, err)
	segments, ok := NodePath(ast.Root())
	require.True(t, ok)
	segments[0].Key = "changed"
	again, ok := NodePath(ast.Root())
	require.True(t, ok)
	require.Equal(t, "request", again[0].Key)
}

func TestASTOperatorValidationPositions(t *testing.T) {
	for _, tc := range []struct {
		expr    string
		line    int
		column  int
		message string
	}{
		{`field > "text"`, 1, 7, "invalid operation"},
		{`field not in 1`, 1, 7, "in requires an array or CIDR value"},
		{`field not matches "text"`, 1, 19, "matches requires a regex value"},
		{"field == 1 and\nother not in 2", 2, 7, "in requires an array or CIDR value"},
		{`field ==`, 1, 9, "empty expression"},
	} {
		t.Run(tc.expr, func(t *testing.T) {
			_, err := ParseAST(tc.expr)
			var parseErr *ParseError
			require.ErrorAs(t, err, &parseErr)
			require.Equal(t, tc.line, parseErr.Line)
			require.Equal(t, tc.column, parseErr.Column)
			require.Equal(t, tc.message, parseErr.Message)
		})
	}
}
