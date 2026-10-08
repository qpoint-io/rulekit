package rulekit

import (
	"context"
	"errors"
	"sync"
	"sync/atomic"
	"testing"
	"time"

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

func TestInputFromKVLazyMemoKeepsBracketKeysDistinct(t *testing.T) {
	lazy := func(v string) LazyValue { return func() (any, error) { return v, nil } }
	input := FromKV(KV{
		"a.b": lazy("flat"),
		"a":   KV{"b": lazy("nested")},
	})
	result := MustParse(`["a.b"] == "flat" and a.b == "nested" and a["b"] == "nested"`).Eval(nil, input, Opts{})
	require.NoError(t, result.Error)
	require.True(t, result.Pass())
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

// A single FromKV input may be shared by concurrent evals (e.g. several rules
// evaluated against one request from different goroutines). Lazy values must
// be resolved exactly once per path and the memo must not race.
func TestInputFromKVConcurrentEval(t *testing.T) {
	type contextKey string
	var plainCalls, ctxCalls, nestedCalls atomic.Int64

	input := FromKV(KV{
		"static": "value",
		"plain": LazyValue(func() (any, error) {
			plainCalls.Add(1)
			time.Sleep(time.Millisecond)
			return "plain", nil
		}),
		"user": LazyContextValue(func(ctx context.Context) (any, error) {
			ctxCalls.Add(1)
			time.Sleep(time.Millisecond)
			return ctx.Value(contextKey("user")), nil
		}),
		"request": KV{
			"id": LazyValue(func() (any, error) {
				nestedCalls.Add(1)
				time.Sleep(time.Millisecond)
				return "req-1", nil
			}),
		},
	})
	rules := []Rule{
		MustParse(`static == "value"`),
		MustParse(`plain == "plain"`),
		MustParse(`user == "root"`),
		MustParse(`request.id == "req-1"`),
		MustParse(`plain == "plain" and user == "root" and request.id == "req-1" and static == "value"`),
	}
	ctx := context.WithValue(context.Background(), contextKey("user"), "root")

	const goroutines = 16
	start := make(chan struct{})
	var wg sync.WaitGroup
	for g := range goroutines {
		wg.Add(1)
		go func() {
			defer wg.Done()
			<-start
			for i := range 50 {
				rule := rules[(g+i)%len(rules)]
				result := rule.Eval(ctx, input, Opts{})
				if result.Error != nil || !result.Pass() {
					t.Errorf("goroutine %d: %s: error=%v pass=%v", g, rule, result.Error, result.Pass())
					return
				}
			}
		}()
	}
	close(start)
	wg.Wait()

	require.EqualValues(t, 1, plainCalls.Load())
	require.EqualValues(t, 1, ctxCalls.Load())
	require.EqualValues(t, 1, nestedCalls.Load())
}

// Failed lazy resolutions are not memoized; a later lookup retries.
func TestInputFromKVLazyErrorNotMemoized(t *testing.T) {
	var calls int
	rule := MustParse(`flaky == "ok"`)
	input := FromKV(KV{
		"flaky": LazyValue(func() (any, error) {
			calls++
			if calls == 1 {
				return nil, errors.New("boom")
			}
			return "ok", nil
		}),
	})

	require.Error(t, rule.Eval(nil, input, Opts{}).Error)
	result := rule.Eval(nil, input, Opts{})
	require.NoError(t, result.Error)
	require.True(t, result.Pass())
	require.Equal(t, 2, calls)
}
