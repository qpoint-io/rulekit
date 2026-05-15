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
		"payload.$bytes_hex": "474554",
		"max.$uint64": "18446744073709551615",
		"nested": {"count.$int64": "42"}
	}`), JSONOptions{AnnotatedKeys: true})
	require.NoError(t, err)

	require.Equal(t, net.ParseIP("1.2.3.4"), kv["src"])
	require.IsType(t, &net.IPNet{}, kv["dst"])
	require.Equal(t, []byte("GET"), kv["payload"])
	require.Equal(t, uint64(18446744073709551615), kv["max"])
	require.Equal(t, int64(42), kv["nested"].(map[string]any)["count"])
}

func TestDecodeJSONTypedValues(t *testing.T) {
	kv, err := DecodeJSON([]byte(`{
		"src": {"$type": "ip", "value": "1.2.3.4"},
		"payload": {"$type": "bytes", "encoding": "base64", "value": "R0VU"},
		"u64": {"$type": "uint64", "value": "18446744073709551615"}
	}`), JSONOptions{})
	require.NoError(t, err)

	require.Equal(t, net.ParseIP("1.2.3.4"), kv["src"])
	require.Equal(t, []byte("GET"), kv["payload"])
	require.Equal(t, uint64(18446744073709551615), kv["u64"])
}

func TestDecodeJSONPlainModeKeepsAnnotatedKeys(t *testing.T) {
	kv, err := DecodeJSON([]byte(`{"src.$ip": "1.2.3.4"}`), JSONOptions{})
	require.NoError(t, err)

	require.Equal(t, "1.2.3.4", kv["src.$ip"])
}

func TestDecodeJSONDuplicateAnnotatedKey(t *testing.T) {
	_, err := DecodeJSON([]byte(`{"src": "plain", "src.$ip": "1.2.3.4"}`), JSONOptions{AnnotatedKeys: true})
	require.EqualError(t, err, `duplicate normalized key "src"`)
}

func TestDecodeJSONEval(t *testing.T) {
	data, err := DecodeJSON([]byte(`{"src.$ip": "1.2.3.4", "items": [{"name": "first"}]}`), JSONOptions{AnnotatedKeys: true})
	require.NoError(t, err)

	assertParseEval(t, `src == 1.2.3.4 and items[0].name == "first"`, kv(data), true)
}
