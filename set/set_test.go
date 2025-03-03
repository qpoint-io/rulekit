package set

import (
	"testing"

	"github.com/stretchr/testify/require"
)

func TestNewSet(t *testing.T) {
	tests := []struct {
		name     string
		items    []int
		expected []int
	}{
		{
			name:     "empty set",
			items:    []int{},
			expected: []int{},
		},
		{
			name:     "single item",
			items:    []int{1},
			expected: []int{1},
		},
		{
			name:     "multiple items",
			items:    []int{1, 2, 3},
			expected: []int{1, 2, 3},
		},
		{
			name:     "duplicate items",
			items:    []int{1, 1, 2, 2, 3},
			expected: []int{1, 2, 3},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			set := NewSet(tt.items...)
			require.ElementsMatch(t, tt.expected, set.Items())
		})
	}
}

func TestSet_Operations(t *testing.T) {
	t.Run("add and contains", func(t *testing.T) {
		set := NewSet[int]()
		set.Add(1, 2, 3)

		require.True(t, set.Contains(1))
		require.True(t, set.Contains(2))
		require.True(t, set.Contains(3))
		require.False(t, set.Contains(4))
	})

	t.Run("remove", func(t *testing.T) {
		set := NewSet(1, 2, 3)
		set.Remove(2)

		require.True(t, set.Contains(1))
		require.False(t, set.Contains(2))
		require.True(t, set.Contains(3))
	})

	t.Run("size", func(t *testing.T) {
		tests := []struct {
			name     string
			set      Set[int]
			expected int
		}{
			{
				name:     "nil set",
				set:      nil,
				expected: 0,
			},
			{
				name:     "empty set",
				set:      NewSet[int](),
				expected: 0,
			},
			{
				name:     "non-empty set",
				set:      NewSet(1, 2, 3),
				expected: 3,
			},
		}

		for _, tt := range tests {
			t.Run(tt.name, func(t *testing.T) {
				require.Len(t, tt.set.Items(), tt.expected)
			})
		}
	})
}

func TestSet_Merge(t *testing.T) {
	tests := []struct {
		name     string
		set1     []int
		set2     []int
		expected []int
	}{
		{
			name:     "merge empty sets",
			set1:     []int{},
			set2:     []int{},
			expected: []int{},
		},
		{
			name:     "merge with empty set",
			set1:     []int{1, 2},
			set2:     []int{},
			expected: []int{1, 2},
		},
		{
			name:     "merge distinct sets",
			set1:     []int{1, 2},
			set2:     []int{3, 4},
			expected: []int{1, 2, 3, 4},
		},
		{
			name:     "merge overlapping sets",
			set1:     []int{1, 2, 3},
			set2:     []int{2, 3, 4},
			expected: []int{1, 2, 3, 4},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			set1 := NewSet(tt.set1...)
			set2 := NewSet(tt.set2...)
			set1.Merge(set2)
			require.ElementsMatch(t, tt.expected, set1.Items())
		})
	}
}

func TestUnion(t *testing.T) {
	tests := []struct {
		name     string
		sets     [][]int
		expected []int
	}{
		{
			name:     "union empty sets",
			sets:     [][]int{{}, {}},
			expected: []int{},
		},
		{
			name:     "union single set",
			sets:     [][]int{{1, 2, 3}},
			expected: []int{1, 2, 3},
		},
		{
			name: "union multiple sets",
			sets: [][]int{
				{1, 2},
				{2, 3},
				{3, 4},
			},
			expected: []int{1, 2, 3, 4},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			var sets []Set[int]
			for _, items := range tt.sets {
				sets = append(sets, NewSet(items...))
			}
			result := Union(sets...)
			require.ElementsMatch(t, tt.expected, result.Items())
		})
	}
}
