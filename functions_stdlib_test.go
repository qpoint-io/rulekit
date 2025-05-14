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
