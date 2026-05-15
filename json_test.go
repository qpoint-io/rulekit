package rulekit

import (
	"net"
	"testing"

	"github.com/stretchr/testify/require"
)

func TestDecodeJSONAnnotatedKeys(t *testing.T) {
	kv, err := DecodeJSON([]byte(`{
		"src.$ip": "1.2.3.4",
		"dst.$cidr": "10.0.0.0/8",
		"payload.$hex": "474554",
		"name.$string": "api",
		"enabled.$bool": true,
		"max.$uint64": "18446744073709551615",
		"nested": {"count.$int64": "42"}
	}`), JSONOptions{AnnotatedKeys: true})
	require.NoError(t, err)

	require.Equal(t, net.ParseIP("1.2.3.4"), kv["src"])
	require.IsType(t, &net.IPNet{}, kv["dst"])
	require.Equal(t, []byte("GET"), kv["payload"])
	require.Equal(t, "api", kv["name"])
	require.Equal(t, true, kv["enabled"])
	require.Equal(t, uint64(18446744073709551615), kv["max"])
	require.Equal(t, int64(42), kv["nested"].(map[string]any)["count"])
}

func TestDecodeJSONTypedDocument(t *testing.T) {
	kv, err := DecodeJSON([]byte(`{
		"src": {"$type": "ip", "value": "1.2.3.4"},
		"payload": {"$type": "bytes", "encoding": "base64", "value": "R0VU"},
		"u64": {"$type": "uint64", "value": "18446744073709551615"},
		"nested": {"$type": "object", "value": {"enabled": {"$type": "bool", "value": true}}},
		"items": {"$type": "array", "value": [{"$type": "string", "value": "first"}]}
	}`), JSONOptions{TypedDocument: true})
	require.NoError(t, err)

	require.Equal(t, net.ParseIP("1.2.3.4"), kv["src"])
	require.Equal(t, []byte("GET"), kv["payload"])
	require.Equal(t, uint64(18446744073709551615), kv["u64"])
	require.Equal(t, true, kv["nested"].(map[string]any)["enabled"])
	require.Equal(t, []any{"first"}, kv["items"])
}

func TestDecodeJSONPlainModeKeepsAnnotatedKeys(t *testing.T) {
	kv, err := DecodeJSON([]byte(`{"src.$ip": "1.2.3.4", "src": {"$type": "ip", "value": "1.2.3.4"}}`), JSONOptions{})
	require.NoError(t, err)

	require.Equal(t, "1.2.3.4", kv["src.$ip"])
	require.Equal(t, map[string]any{"$type": "ip", "value": "1.2.3.4"}, kv["src"])
}

func TestDecodeJSONDuplicateAnnotatedKey(t *testing.T) {
	_, err := DecodeJSON([]byte(`{"src": "plain", "src.$ip": "1.2.3.4"}`), JSONOptions{AnnotatedKeys: true})
	require.EqualError(t, err, `duplicate normalized key "src"`)
}

func TestDecodeJSONTypedDocumentRejectsPlainValuesAndAnnotatedKeys(t *testing.T) {
	_, err := DecodeJSON([]byte(`{"src.$ip": {"$type": "ip", "value": "1.2.3.4"}}`), JSONOptions{TypedDocument: true})
	require.EqualError(t, err, `typed json key "src.$ip" must not use annotated suffix "$ip"`)

	_, err = DecodeJSON([]byte(`{"src": "1.2.3.4"}`), JSONOptions{TypedDocument: true})
	require.EqualError(t, err, `key "src": typed json value must be an object`)

	_, err = DecodeJSON([]byte(`{"src": {"$type": "ip", "value": "1.2.3.4"}}`), JSONOptions{TypedDocument: true, AnnotatedKeys: true})
	require.EqualError(t, err, `json options AnnotatedKeys and TypedDocument are mutually exclusive`)
}

func TestDecodeJSONEval(t *testing.T) {
	data, err := DecodeJSON([]byte(`{"src.$ip": "1.2.3.4", "items": [{"name": "first"}]}`), JSONOptions{AnnotatedKeys: true})
	require.NoError(t, err)

	assertParseEval(t, `src == 1.2.3.4 and items[0].name == "first"`, kv(data), true)
}
