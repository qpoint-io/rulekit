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
