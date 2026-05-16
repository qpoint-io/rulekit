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

type pathInput interface {
	GetPath(context.Context, []pathSegment) (any, bool, error)
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
	segments := make([]pathSegment, 0, len(path))
	for _, segment := range path {
		segments = append(segments, pathSegment{key: segment.Key, index: segment.Index, isIndex: segment.IsIndex, bracket: segment.Bracket})
	}
	return k.GetPath(ctx, segments)
}

func (k *kvInput) GetPath(ctx context.Context, path []pathSegment) (any, bool, error) {
	if k == nil || k.kv == nil || len(path) == 0 {
		return nil, false, nil
	}
	var current any = map[string]any(k.kv)
	for i, segment := range path {
		if nested, ok := current.(Input); ok {
			if nestedPath, ok := nested.(pathInput); ok {
				return nestedPath.GetPath(ctx, path[i:])
			}
			return nested.Get(ctx, inputPathSegments(path[i:]))
		}

		if segment.isIndex {
			value, ok := indexAny(current, segment.index)
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
		value, ok := currentMap[segment.key]
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

func (k *kvInput) resolveLazy(ctx context.Context, path []pathSegment, value any) (any, error) {
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

func inputPathKey(path []pathSegment) string {
	var raw strings.Builder
	for _, segment := range path {
		if segment.isIndex {
			raw.WriteString("[")
			raw.WriteString(strconv.Itoa(segment.index))
			raw.WriteString("]")
			continue
		}
		if raw.Len() > 0 {
			raw.WriteString(".")
		}
		raw.WriteString(segment.key)
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

func resolveInputPath(ctx context.Context, input Input, segments []pathSegment) (any, bool, error) {
	if input == nil {
		return nil, false, nil
	}
	if ctx == nil {
		ctx = context.Background()
	}
	if pathInput, ok := input.(pathInput); ok {
		return pathInput.GetPath(ctx, segments)
	}
	return input.Get(ctx, inputPathSegments(segments))
}

func inputError(field string, err error) Result {
	return Result{Error: fmt.Errorf("field %q: %w", field, err)}
}
