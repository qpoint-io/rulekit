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

// Trailing data cannot be expressed as a test vector (vector inputs are JSON
// objects), so it is pinned here and in the Rust tests.
func TestDecodeJSONRejectsTrailingData(t *testing.T) {
	_, err := DecodeJSON([]byte("{\"a\": 1}  \n\t"), JSONOptions{})
	require.NoError(t, err)

	for _, data := range []string{`{"a": 1} {"b": 2}`, `{"a": 1}x`, `{"a": 1}]`} {
		_, err := DecodeJSON([]byte(data), JSONOptions{})
		require.Error(t, err, data)
	}
}

// Invalid UTF-8 and lone surrogate escapes cannot be expressed as test vectors
// (vector files are valid JSON), so they are pinned here and in the Rust tests.
func TestDecodeJSONRejectsInvalidText(t *testing.T) {
	for _, doc := range []string{
		"{\"a\": \"x\xffy\"}",
		"{\"x\xff\": 1}",
		`{"a": "\ud800"}`,
		`{"a": "\udc00"}`,
		`{"a": "\ude00\ud83d"}`,
		`{"a": "\ud800\u0041"}`,
		`{"\ud800": 1}`,
		`{"a": "\\ud800\udc00"}`,
	} {
		_, err := DecodeJSON([]byte(doc), JSONOptions{})
		require.Error(t, err, "%q", doc)
	}
	for _, doc := range []string{
		`{"a": "\ud83d\ude00"}`,
		`{"a": "\\ud800"}`,
		`{"a": "\u00e9 \"\\"}`,
	} {
		_, err := DecodeJSON([]byte(doc), JSONOptions{})
		require.NoError(t, err, "%q", doc)
	}
}
