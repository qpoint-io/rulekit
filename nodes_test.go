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
		"part.of.the.key":        {"period", true},
		"nested.part.of.the.key": {"period", true},
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
