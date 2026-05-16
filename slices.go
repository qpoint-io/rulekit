package rulekit

func appendUnique[T comparable](items []T, more ...T) []T {
	for _, item := range more {
		seen := false
		for _, existing := range items {
			if existing == item {
				seen = true
				break
			}
		}
		if !seen {
			items = append(items, item)
		}
	}
	return items
}

func unionUnique[T comparable](left, right []T) []T {
	if len(left) == 0 {
		return right
	}
	if len(right) == 0 {
		return left
	}
	items := make([]T, 0, len(left)+len(right))
	items = append(items, left...)
	return appendUnique(items, right...)
}
