package rulekit

import (
	"net"
	"net/url"
	"testing"

	"github.com/stretchr/testify/require"
)

func TestURLFields(t *testing.T) {
	u, err := url.Parse("https://alice@Example.com:8443/api/v1?tag=a&tag=b&env=prod#top")
	require.NoError(t, err)
	bare, err := url.Parse("http://example.com")
	require.NoError(t, err)
	input := kv{"url": u, "bare": bare}

	assertRulep(t, `url.scheme == "https"`, input).Ok().DoesPass(true)
	assertRulep(t, `url.host == "example.com"`, input).Ok().DoesPass(true)
	assertRulep(t, `url.port == 8443`, input).Ok().DoesPass(true)
	assertRulep(t, `url.path == "/api/v1"`, input).Ok().DoesPass(true)
	assertRulep(t, `url.query.env == "prod"`, input).Ok().DoesPass(true)
	assertRulep(t, `url.query["tag"] == "b"`, input).Ok().DoesPass(true)
	assertRulep(t, `url.fragment == "top"`, input).Ok().DoesPass(true)
	assertRulep(t, `url.user == "alice"`, input).Ok().DoesPass(true)

	// Parts absent from the URL are unknown, not empty.
	assertRulep(t, `bare.port == 80`, input).MissingFields("bare.port").Value(nil)
	assertRulep(t, `bare.query.env == "prod"`, input).MissingFields("bare.query.env").Value(nil)
	assertRulep(t, `bare.fragment == ""`, input).MissingFields("bare.fragment").Value(nil)
	assertRulep(t, `bare.user == ""`, input).MissingFields("bare.user").Value(nil)
	assertRulep(t, `bare.path == ""`, input).Ok().DoesPass(true)
	assertRulep(t, `url.nope == 1`, input).MissingFields("url.nope").Value(nil)
}

func TestURLValueComparesAsString(t *testing.T) {
	u, err := url.Parse("https://example.com/a")
	require.NoError(t, err)
	input := kv{"url": u, "s": "https://example.com/a"}

	assertRulep(t, `url == "https://example.com/a"`, input).Ok().DoesPass(true)
	assertRulep(t, `url != "https://example.com/b"`, input).Ok().DoesPass(true)
	assertRulep(t, `url contains "example.com"`, input).Ok().DoesPass(true)
	assertRulep(t, `url matches /^https:/`, input).Ok().DoesPass(true)
	assertRulep(t, `url in ["https://example.com/b", "https://example.com/a"]`, input).Ok().DoesPass(true)
	// A KV string stays a string: it compares to URL literals by text but has no fields.
	assertRulep(t, `s == "https://example.com/a"`, input).Ok().DoesPass(true)
	assertRulep(t, `s.host == "example.com"`, input).MissingFields("s.host").Value(nil)
}

func TestQuotedURLLiteral(t *testing.T) {
	for raw, isURL := range map[string]bool{
		"https://example.com/a?b=c": true,
		"http://[::1]:8080/":        true,
		"mailto:a@example.com":      false,
		"https://":                  false,
		"example.com/a":             false,
		// Literals whose canonical form differs from the source stay strings,
		// so comparisons against KV strings keep matching the text as written.
		"https://example.com/a b": false,
	} {
		value, err := parseString(`"` + raw + `"`)
		require.NoError(t, err, raw)
		_, gotURL := value.(*url.URL)
		require.Equal(t, isURL, gotURL, raw)
	}
}

func TestIPFields(t *testing.T) {
	input := kv{
		"v4":     net.ParseIP("10.1.2.3"),
		"v6":     net.ParseIP("2001:db8::1"),
		"mapped": net.ParseIP("::ffff:10.1.2.3"),
	}
	assertRulep(t, `v4.version == "v4"`, input).Ok().DoesPass(true)
	assertRulep(t, `v6.version == "v6"`, input).Ok().DoesPass(true)
	assertRulep(t, `mapped.version == "v4"`, input).Ok().DoesPass(true)
	assertRulep(t, `v4.nope == 1`, input).MissingFields("v4.nope").Value(nil)
}

func TestCIDRFields(t *testing.T) {
	_, v4, err := net.ParseCIDR("10.1.0.0/16")
	require.NoError(t, err)
	_, v6, err := net.ParseCIDR("2001:db8::/32")
	require.NoError(t, err)
	input := kv{"net4": v4, "net6": v6}

	assertRulep(t, `net4.network == 10.1.0.0`, input).Ok().DoesPass(true)
	assertRulep(t, `net4.prefix == 16`, input).Ok().DoesPass(true)
	assertRulep(t, `net4.version == "v4"`, input).Ok().DoesPass(true)
	assertRulep(t, `net6.prefix == 32`, input).Ok().DoesPass(true)
	assertRulep(t, `net6.version == "v6"`, input).Ok().DoesPass(true)
}

func TestMACFields(t *testing.T) {
	mac, err := net.ParseMAC("00:1A:2B:3C:4D:5E")
	require.NoError(t, err)
	input := kv{"mac": mac}

	assertRulep(t, `mac.oui == 00:1a:2b`, input).Ok().DoesPass(true)
	assertRulep(t, `mac.oui == "00:1a:2b"`, input).Ok().DoesPass(true)
	assertRulep(t, `mac.oui == 00:1a:2c`, input).Ok().DoesPass(false)
}

func TestValueFieldsMapKeysWin(t *testing.T) {
	// Fields only apply to typed values; a map with the same key is read as a map.
	assertRulep(t, `url.host == "from-map"`, kv{"url": KV{"host": "from-map"}}).Ok().DoesPass(true)
}

func TestDecodeJSONURL(t *testing.T) {
	data, err := DecodeJSON([]byte(`{"a.$url": "https://example.com:8443/x"}`), JSONOptions{AnnotatedKeys: true})
	require.NoError(t, err)
	assertRulep(t, `a.port == 8443 and a.host == "example.com"`, kv(data)).Ok().DoesPass(true)

	typed, err := DecodeJSON([]byte(`{"a": {"$type": "url", "value": "https://example.com/x"}}`), JSONOptions{TypedDocument: true})
	require.NoError(t, err)
	assertRulep(t, `a.path == "/x"`, kv(typed)).Ok().DoesPass(true)
}
