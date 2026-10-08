package rulekit

import (
	"testing"

	"github.com/stretchr/testify/assert"
)

func Test_mapPath(t *testing.T) {
	m := map[string]any{
		"part.of.the.key": "period",
		"nested": map[string]any{
			"part.of.the.key": "period",
		},
		"src": map[string]any{
			"trusted": true,
			"process": map[string]any{
				"name": "qpoint",
				"path": "/usr/bin/qpoint",
			},
		},
		"dst": map[string]any{
			"host": "192.168.1.1",
			"port": 8080,
		},
	}

	for key, want := range map[string]struct {
		val any
		ok  bool
	}{
		"part.of.the.key":        {nil, false},
		"nested.part.of.the.key": {nil, false},
		"src.process": {
			val: map[string]any{
				"name": "qpoint",
				"path": "/usr/bin/qpoint",
			},
			ok: true,
		},
		"src.process.name":     {"qpoint", true},
		"src.process.path":     {"/usr/bin/qpoint", true},
		"src.process.path.idk": {nil, false},
		"src.trusted":          {true, true},
		"src.trusted.idk":      {nil, false},

		"dst.host": {"192.168.1.1", true},
		"dst.port": {8080, true},
	} {
		got, ok := IndexKV(m, key)
		assert.Equal(t, want, struct {
			val any
			ok  bool
		}{got, ok}, key)
	}
}

// Go input can hold typed slices that the JSON vectors cannot express.
func TestTypedSliceTruthiness(t *testing.T) {
	rule := MustParse(`tags || "none"`)
	for _, tc := range []struct {
		tags any
		want any
	}{
		{[]string{}, "none"},
		{[]string(nil), "none"},
		{[]int{}, "none"},
		{[]string{"a"}, []string{"a"}},
	} {
		res := rule.Eval(nil, FromKV(KV{"tags": tc.tags}), Opts{})
		assert.Equal(t, tc.want, res.Value, "%#v", tc.tags)
	}
}
