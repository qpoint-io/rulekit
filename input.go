package rulekit

import (
	"context"
	"fmt"
	"sync"
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
	return &kvInput{kv: kv}
}

// kvInput may be shared by concurrent evals; mu guards memo, and each
// lazyEntry guards its own resolution.
type kvInput struct {
	kv   KV
	mu   sync.Mutex
	memo map[string]*lazyEntry
}

type lazyEntry struct {
	mu    sync.Mutex
	done  bool
	value any
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
			field, ok := valueField(current, segment.key)
			if !ok {
				return nil, false, nil
			}
			current = field
			continue
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

// getField resolves a single top-level key. It is GetPath for one segment
// without building a path slice, which keeps plain field reads allocation-free.
func (k *kvInput) getField(ctx context.Context, key string) (any, bool, error) {
	if k == nil || k.kv == nil {
		return nil, false, nil
	}
	value, ok := k.kv[key]
	if !ok {
		return nil, false, nil
	}
	switch value.(type) {
	case LazyValue, LazyContextValue:
		resolved, err := k.resolveLazy(ctx, []pathSegment{{key: key}}, value)
		if err != nil {
			return nil, false, err
		}
		return resolved, true, nil
	}
	return value, true, nil
}

func (k *kvInput) resolveLazy(ctx context.Context, path []pathSegment, value any) (any, error) {
	// Plain values never touch the memo, so the common path stays lock-free.
	switch value.(type) {
	case LazyValue, LazyContextValue:
	default:
		return value, nil
	}

	// A kvInput may be shared by concurrent evals. The map lock is held only to
	// find the per-path entry; the entry lock is held across the lazy call so
	// each path resolves once and lazy funcs resolving other paths on the same
	// input do not block on each other.
	key := pathString(path, false)
	k.mu.Lock()
	entry := k.memo[key]
	if entry == nil {
		if k.memo == nil {
			k.memo = map[string]*lazyEntry{}
		}
		entry = &lazyEntry{}
		k.memo[key] = entry
	}
	k.mu.Unlock()

	entry.mu.Lock()
	defer entry.mu.Unlock()
	if entry.done {
		return entry.value, nil
	}

	var (
		resolved any
		err      error
	)
	switch v := value.(type) {
	case LazyValue:
		resolved, err = v()
	case LazyContextValue:
		resolved, err = v(ctx)
	}
	if err != nil {
		return nil, err
	}
	entry.value, entry.done = resolved, true
	return resolved, nil
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
