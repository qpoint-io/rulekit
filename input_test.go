package rulekit

import (
	"context"
	"testing"

	"github.com/stretchr/testify/require"
)

func TestInputFromKVLazyValue(t *testing.T) {
	var calls int
	rule := MustParse(`expensive == "value" and expensive == "value"`)
	input := FromKV(KV{
		"expensive": LazyValue(func() (any, error) {
			calls++
			return "value", nil
		}),
	})

	result := rule.Eval(nil, input, Opts{})
	require.NoError(t, result.Error)
	require.True(t, result.Pass())
	require.Equal(t, 1, calls)
}

func TestInputFromKVLazyContextValue(t *testing.T) {
	type contextKey string
	rule := MustParse(`user == "root"`)
	input := FromKV(KV{
		"user": LazyContextValue(func(ctx context.Context) (any, error) {
			return ctx.Value(contextKey("user")), nil
		}),
	})
	ctx := context.WithValue(context.Background(), contextKey("user"), "root")

	result := rule.Eval(ctx, input, Opts{})
	require.NoError(t, result.Error)
	require.True(t, result.Pass())
}

func TestInputNestedInputTakesOverSubtree(t *testing.T) {
	rule := MustParse(`request.headers["user-agent"] == "curl"`)
	requestInput := FromFunc(func(path []PathSegment) (any, bool, error) {
		require.Equal(t, []PathSegment{{Key: "headers"}, {Key: "user-agent", Bracket: true}}, path)
		return "curl", true, nil
	})
	input := FromKV(KV{"request": requestInput})

	result := rule.Eval(nil, input, Opts{})
	require.NoError(t, result.Error)
	require.True(t, result.Pass())
}
