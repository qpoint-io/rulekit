package set

import (
	"golang.org/x/exp/maps"
)

type Set[T comparable] map[T]struct{}

func NewSet[T comparable](items ...T) Set[T] {
	return NewSetSize(len(items), items...)
}

func NewSetSize[T comparable](size int, items ...T) Set[T] {
	set := make(Set[T], size)
	set.Add(items...)
	return set
}

func (s Set[T]) Add(items ...T) {
	for _, item := range items {
		s[item] = struct{}{}
	}
}

func (s Set[T]) Contains(item T) bool {
	_, ok := s[item]
	return ok
}

func (s Set[T]) Remove(item T) {
	delete(s, item)
}

func (s Set[T]) Items() []T {
	if len(s) == 0 {
		return nil
	}
	return maps.Keys(s)
}

func (s Set[T]) Merge(other Set[T]) {
	s.Add(other.Items()...)
}

func (s Set[T]) Len() int {
	return len(s)
}

func Union[T comparable](sets ...Set[T]) Set[T] {
	var union Set[T]
	for _, set := range sets {
		if set == nil {
			continue
		}
		if union == nil {
			union = NewSetSize[T](len(set))
		}
		union.Merge(set)
	}
	return union
}
