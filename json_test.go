package rulekit

import (
	"testing"

	"github.com/stretchr/testify/require"
)

// TestDecodeJSONErrors pins the Go error messages and the Go options struct.
// Which inputs fail is covered by testdata/vectors/json_input.json.
func TestDecodeJSONErrors(t *testing.T) {
	_, err := DecodeJSON([]byte(`{"src": "plain", "src.$ip": "1.2.3.4"}`), JSONOptions{AnnotatedKeys: true})
	require.EqualError(t, err, `duplicate normalized key "src"`)

	_, err = DecodeJSON([]byte(`{"src.$ip": {"$type": "ip", "value": "1.2.3.4"}}`), JSONOptions{TypedDocument: true})
	require.EqualError(t, err, `typed json key "src.$ip" must not use annotated suffix "$ip"`)

	_, err = DecodeJSON([]byte(`{"src": "1.2.3.4"}`), JSONOptions{TypedDocument: true})
	require.EqualError(t, err, `key "src": typed json value must be an object`)

	_, err = DecodeJSON([]byte(`{"src": {"$type": "ip", "value": "1.2.3.4"}}`), JSONOptions{TypedDocument: true, AnnotatedKeys: true})
	require.EqualError(t, err, `json options AnnotatedKeys and TypedDocument are mutually exclusive`)
}
