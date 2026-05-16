package rulekit

import (
	"context"
	"fmt"
	"strconv"
	"strings"
)

// Input resolves rule paths against an evaluation input source.
type Input interface {
	Get(context.Context, []PathSegment) (any, bool, error)
}

type LazyValue func() (any, error)
type LazyContextValue func(context.Context) (any, error)

type inputFunc func(context.Context, []PathSegment) (any, bool, error)

func (f inputFunc) Get(ctx context.Context, path []PathSegment) (any, bool, error) {
	return f(ctx, path)
}

func FromFunc(fn func([]PathSegment) (any, bool, error)) Input {
	return inputFunc(func(_ context.Context, path []PathSegment) (any, bool, error) {
		return fn(path)
	})
}

func FromContextFunc(fn func(context.Context, []PathSegment) (any, bool, error)) Input {
	return inputFunc(fn)
}

func FromKV(kv KV) Input {
	return &kvInput{kv: kv, memo: map[string]any{}}
}

type kvInput struct {
	kv   KV
	memo map[string]any
}

func (k *kvInput) Get(ctx context.Context, path []PathSegment) (any, bool, error) {
	if k == nil || k.kv == nil || len(path) == 0 {
		return nil, false, nil
	}
	var current any = map[string]any(k.kv)
	for i, segment := range path {
		if nested, ok := current.(Input); ok {
			return nested.Get(ctx, path[i:])
		}

		if segment.IsIndex {
			value, ok := indexAny(current, segment.Index)
			if !ok {
				return nil, false, nil
			}
			current = value
			continue
		}

		currentMap, ok := current.(map[string]any)
		if !ok {
			return nil, false, nil
		}
		value, ok := currentMap[segment.Key]
		if !ok {
			return nil, false, nil
		}
		resolved, err := k.resolveLazy(ctx, path[:i+1], value)
		if err != nil {
			return nil, false, err
		}
		current = resolved
	}
	return current, true, nil
}

func (k *kvInput) resolveLazy(ctx context.Context, path []PathSegment, value any) (any, error) {
	key := inputPathKey(path)
	if resolved, ok := k.memo[key]; ok {
		return resolved, nil
	}

	var (
		resolved any
		err      error
		lazy     bool
	)
	switch v := value.(type) {
	case LazyValue:
		resolved, err = v()
		lazy = true
	case LazyContextValue:
		resolved, err = v(ctx)
		lazy = true
	default:
		return value, nil
	}
	if err != nil {
		return nil, err
	}
	if lazy {
		k.memo[key] = resolved
	}
	return resolved, nil
}

func inputPathKey(path []PathSegment) string {
	var raw strings.Builder
	for _, segment := range path {
		if segment.IsIndex {
			raw.WriteString("[")
			raw.WriteString(strconv.Itoa(segment.Index))
			raw.WriteString("]")
			continue
		}
		if raw.Len() > 0 {
			raw.WriteString(".")
		}
		raw.WriteString(segment.Key)
	}
	return raw.String()
}

func inputPathSegments(segments []pathSegment) []PathSegment {
	out := make([]PathSegment, 0, len(segments))
	for _, segment := range segments {
		out = append(out, PathSegment{Key: segment.key, Index: segment.index, IsIndex: segment.isIndex, Bracket: segment.bracket})
	}
	return out
}

func resolveInputPath(ctx *Ctx, segments []pathSegment) (any, bool, error) {
	if ctx == nil {
		return nil, false, nil
	}
	input := ctx.valueInput()
	if input == nil {
		return nil, false, nil
	}
	return input.Get(ctx.context(), inputPathSegments(segments))
}

func usesInput(ctx *Ctx) bool {
	return ctx != nil && (ctx.Input != nil || ctx.input != nil || ctx.Context != nil)
}

func inputError(field string, err error) Result {
	return Result{Error: fmt.Errorf("field %q: %w", field, err)}
}
