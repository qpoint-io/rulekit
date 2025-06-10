package rulekit

import (
	"errors"
	"net"
	"testing"

	"github.com/stretchr/testify/require"
)

func TestFn_InvalidArgs(t *testing.T) {
	_, err := Parse("some_none_stdlib_fn()")
	require.NoError(t, err)

	assertRulep(t, "unknown_fn()", nil).Error(errors.New(`unknown function "unknown_fn"`))
	assertRulep(t, "unknown_fn(some_args)", nil).Error(errors.New(`unknown function "unknown_fn"`))
}

func TestFn_StartsWith(t *testing.T) {
	r := MustParse(`starts_with(url, "https://")`)
	assertRule(t, r, kv{"url": "https://example.com"}).Pass()
	assertRule(t, r, kv{"url": "http://example.com"}).Fail()
	assertRule(t, r, kv{"url": "invalid-url"}).Fail()

	// non-string args
	assertRulep(t, `starts_with(ip, "10.0")`, kv{"ip": net.ParseIP("10.0.0.1")}).Pass()
	assertRulep(t, `starts_with(code, 5)`, kv{"code": 500}).Pass()
	assertRulep(t, `starts_with(code, "5")`, kv{"code": 500}).Pass()
	assertRulep(t, `starts_with(code, 5)`, kv{"code": 404}).Fail()
	assertRulep(t, `starts_with(starts_with("https://example.com", "https://"), "true")`, nil).Pass()

	// parser errors
	assertParseErrorValue(t, "starts_with()", `syntax error at line 1:14:
starts_with()
             ^
function "starts_with" expects 2 arguments, got 0`)
	assertParseErrorValue(t, "starts_with(arg1)", `syntax error at line 1:18:
starts_with(arg1)
                 ^
function "starts_with" expects 2 arguments, got 1`)
}

func TestFn_Index(t *testing.T) {
	// happy path - map
	assertRulep(t,
		`index(map, "key")`,
		kv{"map": KV{"key": "value"}},
	).Value("value")

	// happy path - array
	assertRulep(t, `index([1, 2, 3], 0)`, nil).Value(int64(1))

	// happy path - nested map
	assertRulep(t,
		`index(map, "key.nested")`,
		kv{"map": KV{"key": KV{"nested": "value"}}},
	).Value("value")
	assertRulep(t,
		`index(index(map, "key"), "nested")`,
		kv{"map": KV{"key": KV{"nested": "value"}}},
	).Value("value")

	// int key with map
	assertRulep(t,
		`index(map, 123)`,
		kv{"map": KV{"key": "value"}},
	).ErrorString(`arg key: expected string, got int64`)

	// string key with array
	assertRulep(t,
		`index([1, 2, 3], "test")`,
		kv{"map": []any{1, 2, 3}},
	).ErrorString(`arg key: expected int64, got string`)

	// out of bounds key with array
	assertRulep(t,
		`index([1, 2, 3], 10)`,
		kv{"map": []any{1, 2, 3}},
	).ErrorString(`index 10 out of bounds`)
	assertRulep(t,
		`index([1, 2, 3], -3)`,
		kv{"map": []any{1, 2, 3}},
	).ErrorString(`index -3 out of bounds`)

	// invalid container type
	assertRulep(t,
		`index(map, "test")`,
		kv{"map": 123},
	).ErrorString(`container must be a map or array`)
}
