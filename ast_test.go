package rulekit

import (
	"net"
	"testing"

	"github.com/stretchr/testify/require"
)

func TestPrintASTCompact(t *testing.T) {
	tests := map[string]string{
		`field == "value" and other != 1`:               `field == "value" and other != 1`,
		`domain matches /example\.com$/ OR tags == "x"`: `domain =~ /example\.com$/ or tags == "x"`,
		`a or (b and c)`:                                `a or b and c`,
		`(a or b) and c`:                                `(a or b) and c`,
		`request.headers["user-agent"] == "curl"`:       `request.headers["user-agent"] == "curl"`,
		`items[0].name in ["first", "second"]`:          `items[0].name in ["first", "second"]`,
		`starts_with(path, "/api")`:                     `starts_with(path, "/api")`,
	}

	for input, want := range tests {
		t.Run(input, func(t *testing.T) {
			ast, err := parseAST(input)
			require.NoError(t, err)
			require.Equal(t, want, printAST(ast))
		})
	}
}

func TestPrintASTSemanticRoundTrip(t *testing.T) {
	expressions := []string{
		`field == "value" and other != 1`,
		`domain matches /example\.com$/ OR tags == "x"`,
		`(a or b) and c`,
		`not (a == 1 or b == 2)`,
		`request.headers["user-agent"] == "curl"`,
		`items[0].name in ["first", "second"]`,
		`ip in 192.168.0.0/16`,
		`starts_with(path, "/api")`,
	}

	for _, input := range expressions {
		t.Run(input, func(t *testing.T) {
			ast, err := parseAST(input)
			require.NoError(t, err)

			printed := printAST(ast)
			roundTripped, err := parseAST(printed)
			require.NoError(t, err)

			originalRule, err := lowerAST(ast)
			require.NoError(t, err)
			printedRule, err := lowerAST(roundTripped)
			require.NoError(t, err)
			require.Equal(t, originalRule.String(), printedRule.String())
		})
	}
}

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

func TestFormat(t *testing.T) {
	ast, err := ParseAST(`(a == 1 or b == 2) and c == 3 and request.headers["user-agent"] == "curl"`)
	require.NoError(t, err)

	compact, err := Format(ast, FormatOptions{Mode: FormatCompact})
	require.NoError(t, err)
	require.Equal(t, `(a == 1 or b == 2) and c == 3 and request.headers["user-agent"] == "curl"`, compact)

	multiline, err := Format(ast, FormatOptions{Mode: FormatMultiline, Indent: "    "})
	require.NoError(t, err)
	require.Equal(t, `(a == 1 or b == 2)
and c == 3
and request.headers["user-agent"] == "curl"`, multiline)

	roundTrip, err := ParseAST(multiline)
	require.NoError(t, err)
	require.Equal(t, compact, roundTrip.String())
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
	}}, FormatOptions{Mode: FormatCompact})
	require.NoError(t, err)
	require.Equal(t, "field == 3 -- keep\n and other == 2", rewritten)

	roundTrip, err := ParseAST(rewritten)
	require.NoError(t, err)
	require.Equal(t, `field == 3 and other == 2`, roundTrip.String())
}

func TestCompilePlan(t *testing.T) {
	plan, err := ParsePlan(`ip in 192.168.0.0/16 and request.headers["user-agent"] == "curl"`)
	require.NoError(t, err)
	require.Equal(t, `ip == 192.168.0.0/16 and request.headers["user-agent"] == "curl"`, plan.String())

	result := plan.Eval(&Ctx{KV: KV{
		"ip": net.ParseIP("192.168.1.1"),
		"request": KV{
			"headers": KV{"user-agent": "curl"},
		},
	}})
	require.NoError(t, result.Error)
	require.True(t, result.Pass())
}
