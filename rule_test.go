package rulekit

import (
	"encoding/json"
	"fmt"
	"net"
	"os"
	"reflect"
	"slices"
	"strings"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func init() {
	// SetDebugLevel(5)
	// SetErrorVerbose(true)
}

// TestResult mirrors Result but with EvaluatedRule as a string for easier testing
type TestResult struct {
	Pass          bool
	MissingFields []string
	EvaluatedRule string // stores the string representation of the rule
}

// toTestResult converts a Result to TestResult for easier test assertions
func toTestResult(r Result) TestResult {
	mf := r.MissingFields.Items()
	slices.Sort(mf)
	tr := TestResult{
		Pass:          r.Pass,
		MissingFields: mf,
	}
	if r.EvaluatedRule != nil {
		tr.EvaluatedRule = r.EvaluatedRule.String()
	}
	return tr
}

func TestEngineExample(t *testing.T) {
	filter, err := Parse(`
		tags == 'db-svc'
		OR domain matches /example\.com$/ -- any domain or subdomain of example.com
		OR src.process.path matches |^/usr/bin/| -- patterns can be enclosed in |...| or /.../
		OR (process.uid != 0 AND tags contains 'internal-svc') 
		/* connections to LAN addresses over privileged ports */
		OR (destination.port <= 1023 AND destination.ip == 192.168.0.0/16)
	`)
	require.NoError(t, err)

	assert.True(t, filter.Eval(map[string]any{
		"tags":   []string{"db-svc", "internal-vlan", "unprivileged-user"},
		"domain": "example.com",
		"process": map[string]any{
			"uid":  1000,
			"path": "/usr/bin/some-other-process",
		},
		"port": 8080,
	}).Pass)

	assert.True(t, filter.Eval(map[string]any{
		"destination": map[string]any{
			"ip":   net.ParseIP("192.168.2.37"),
			"port": 22,
		},
	}).Pass)

	assert.False(t, filter.Eval(map[string]any{
		"destination.ip":   net.ParseIP("1.1.1.1"),
		"destination.port": 22,
	}).Pass)

	assert.True(t, filter.Eval(map[string]any{
		"src.process.path": "/usr/bin/some-other-process",
	}).Pass)

	assert.False(t, filter.Eval(map[string]any{
		"src.process.path": "/opt/go",
	}).Pass)
}

func TestEval(t *testing.T) {
	tcs := []struct {
		filter  string
		tests   map[*map[string]any]TestResult
		wantErr string
	}{
		{
			filter: `tls_version == 1.2`,
			tests: map[*map[string]any]TestResult{
				{"tls_version": 1.2}: {
					Pass:          true,
					EvaluatedRule: "tls_version == 1.2",
				},
				{"tls_version": 1.1}: {
					Pass:          false,
					EvaluatedRule: "tls_version == 1.2",
				},
				{}: {
					Pass:          false,
					MissingFields: []string{"tls_version"},
					EvaluatedRule: "tls_version == 1.2",
				},
			},
		},
		{
			filter: `tls_version != 5`,
			tests: map[*map[string]any]TestResult{
				// special handling for the != operator returns true even if the field is missing
				{}: {
					Pass:          true,
					MissingFields: []string{"tls_version"},
					EvaluatedRule: "tls_version != 5",
				},
			},
		},
		{
			filter: `!tls_version`,
			tests: map[*map[string]any]TestResult{
				// special handling for the !FIELD operator returns true even if the field is missing
				{}: {
					Pass:          true,
					MissingFields: []string{"tls_version"},
					EvaluatedRule: "!tls_version",
				},
			},
		},
		{
			filter: `domain matches /example\.com$/ OR tags == "db-svc"`,
			tests: map[*map[string]any]TestResult{
				{"domain": "example.com"}: {
					Pass:          true,
					EvaluatedRule: `domain =~ /example\.com$/`,
				},
				{"tags": "db-svc"}: {
					Pass:          true,
					EvaluatedRule: `tags == "db-svc"`,
				},
				{"domain": "other.com"}: {
					Pass:          false,
					EvaluatedRule: `domain =~ /example\.com$/ or tags == "db-svc"`,
					MissingFields: []string{"tags"},
				},
			},
		},
		{
			filter: `domain == "example.com" AND tags == "db-svc"`,
			tests: map[*map[string]any]TestResult{
				{"domain": "example.com"}: {
					Pass:          false,
					EvaluatedRule: `domain == "example.com" and tags == "db-svc"`,
					MissingFields: []string{"tags"},
				},
				{"tags": "db-svc"}: {
					Pass:          false,
					EvaluatedRule: `domain == "example.com" and tags == "db-svc"`,
					MissingFields: []string{"domain"},
				},
				{"domain": "example.com", "tags": []string{"test", "db-svc"}}: {
					Pass:          true,
					EvaluatedRule: `domain == "example.com" and tags == "db-svc"`,
				},
				{"domain": "qpoint.io"}: {
					Pass:          false,
					EvaluatedRule: `domain == "example.com"`,
				},
				{"tags": []string{}}: {
					Pass:          false,
					EvaluatedRule: `tags == "db-svc"`,
				},
			},
		},
		{
			filter: `
				dst.ip == 8.8.8.8
				or (
					dst.ip == 1.1.1.1
					and (
						dst.port == 53
						or (dst.port == 443 and tls.enabled)
					)
				)`,
			tests: map[*map[string]any]TestResult{
				{
					"tls.enabled": true,
					"dst.ip":      net.ParseIP("1.1.1.1"),
					"dst.port":    443,
				}: {
					Pass:          true,
					EvaluatedRule: `dst.ip == 1.1.1.1 and (dst.port == 443 and tls.enabled)`,
				},
			},
		},
	}

	for _, tc := range tcs {
		p, err := Parse(tc.filter)
		if tc.wantErr != "" {
			require.EqualError(t, err, tc.wantErr)
			return
		}
		require.NoError(t, err)

		for input, want := range tc.tests {
			got := toTestResult(p.Eval(*input))
			if !reflect.DeepEqual(got, want) {
				// print debug info
				SetDebugWriter(&testWriter{t})
				defer SetDebugWriter(os.Stderr)
				SetDebugLevel(1)
				defer SetDebugLevel(0)

				t.Errorf("Filter: %q\nInput: %+v\nGot:  %+v\nWant: %+v", tc.filter, *input, got, want)
			}
		}
	}
}

func BenchmarkParse(b *testing.B) {
	b.Run("simple", func(b *testing.B) {
		for range b.N {
			_, _ = Parse("tags eq 'db-svc'")
		}
	})

	b.Run("complex", func(b *testing.B) {
		for range b.N {
			_, _ = Parse(`tags eq 'db-svc' OR domain matches /example\.com$/ OR (process.uid != 0 AND tags contains 'internal-svc')`)
		}
	})
}

func BenchmarkEval(b *testing.B) {
	simpleFilter, err := Parse("tags eq 'db-svc'")
	require.NoError(b, err)
	largeFilter, err := Parse(`tags eq 'db-svc' OR domain matches /example\.com$/ OR (process.uid != 0 AND tags contains 'internal-svc') OR (destination.port <= 1023 AND destination.ip != 192.168.0.0/16)`)
	require.NoError(b, err)

	smallInput := map[string]any{"tags": "db-svc"}
	largeInput := map[string]any{"tags": []string{"db-svc", "internal-vlan", "unprivileged-user"}, "domain": "example.com", "process.uid": 1000, "port": 8080, "destination.ip": net.ParseIP("192.168.2.37"), "destination.port": 8080}

	b.Run("simple", func(b *testing.B) {
		b.Run("small input", func(b *testing.B) {
			for range b.N {
				simpleFilter.Eval(smallInput)
			}
		})
		b.Run("large input", func(b *testing.B) {
			for range b.N {
				simpleFilter.Eval(largeInput)
			}
		})
	})

	b.Run("complex", func(b *testing.B) {
		b.Run("small input", func(b *testing.B) {
			for range b.N {
				largeFilter.Eval(smallInput)
			}
		})
		b.Run("large input", func(b *testing.B) {
			for range b.N {
				largeFilter.Eval(largeInput)
			}
		})
	})
}

func TestFilterNotZero(t *testing.T) {
	ip, ipnet, err := net.ParseCIDR("1.2.3.4/24")
	require.NoError(t, err)
	mac, err := net.ParseMAC("01:23:45:67:89:ab")
	require.NoError(t, err)

	values := map[string]any{
		"zeroInt":    0,
		"zeroUint":   uint(0),
		"zeroFloat":  float64(0),
		"zeroString": "",
		"zeroBytes":  []byte{},
		"zeroIP":     net.IP{},
		"zeroIPNet":  &net.IPNet{},
		"zeroMac":    net.HardwareAddr{},

		"int":   int(1),
		"uint":  uint64(123414),
		"str":   "hello",
		"bytes": []byte{1, 2, 3},
		"ip":    ip,
		"ipnet": ipnet,
		"mac":   mac,
	}

	for expr, want := range map[string]bool{
		"unset_field": false,
		"zeroNum":     false,
		"zeroString":  false,
		"zeroBytes":   false,
		"zeroIP":      false,
		"zeroIPNet":   false,
		"zeroMac":     false,

		"int":   true,
		"uint":  true,
		"str":   true,
		"bytes": true,
		"ip":    true,
		"ipnet": true,
		"mac":   true,

		"unset_field || ip": true,
		"zeroNum || zeroIP": false,
		"int && mac":        true,
	} {
		r, err := Parse(expr)
		require.NoError(t, err)
		assert.Equal(t, want, r.Eval(values).Pass, expr)
	}
}

func TestFilterParseUint(t *testing.T) {
	_, err := Parse("f_uint==4294967295 && f_uint64==18446744073709551615")
	if err != nil {
		t.Error(err)
	}
	_, err = Parse("f_uint==0 && f_uint64==0")
	if err != nil {
		t.Error(err)
	}
}

func TestFilterParseInt(t *testing.T) {
	_, err := Parse("f_int==2147483647 && f_int64==9223372036854775807")
	if err != nil {
		t.Error(err)
	}
	_, err = Parse("f_int==-2147483648 && f_int64==-9223372036854775808")
	if err != nil {
		t.Error(err)
	}
}

func TestFilterParseString(t *testing.T) {
	_, err := Parse(`f_string=="text" or f_string=="te\"x't" or f_string =='test' or f_string == 'te"s\'t' or f_string contains 12 && f_string==01:23:45:67:89:ab:AB:cd:ef`)
	if err != nil {
		t.Error(err)
	}
}

func TestFilterParseRegexp(t *testing.T) {
	_, err := Parse("f_string matches /gl=se$/ and str matches |some/path/here|")
	if err != nil {
		t.Error(err)
	}
}

func TestFilterParseIP(t *testing.T) {
	_, err := Parse("f_ipv4 == 192.168.1.1 or f_ipv6==::1 or f_ipv6==2001:db8::1")
	if err != nil {
		t.Error(err)
	}
}

func TestFilterParseMac(t *testing.T) {
	_, err := Parse("f_mac == 01:23:45:67:89:ab:cd:ef --or f_mac == 0123.4567.89ab.cdef")
	if err != nil {
		t.Error(err)
	}
}

func TestFilterParseBool(t *testing.T) {
	_, err := Parse("f_bool.1 == true or f_bool.2 != false")
	if err != nil {
		t.Error(err)
	}
}

func TestFilterOperationFieldTypes(t *testing.T) {
	// want error
	for _, s := range []string{
		"f == 123",
		"f == 123",
		"f == 123",
		"f == 123",
		"f == 123",
	} {
		_, err := Parse(s)
		if err != nil {
			t.Errorf("expected error for %s", s)
		}
	}

	// want no error
	for _, s := range []string{
		"f == 123",
		"f == 123",
		"f == 123",
		"f == 123",
		"f == 123",
	} {
		_, err := Parse(s)
		if err != nil {
			t.Errorf("unexpected error for %s", s)
		}
	}
}

func TestFilterParseFloat(t *testing.T) {
	_, err := Parse("f_float32 == 123.345 or f_float64 != 74123412341234.123412341243")
	if err != nil {
		t.Error(err)
	}
}

// Test filters
func TestFilterMatchIntUint(t *testing.T) {
	f, err := Parse("f_int == 1 and f_uint == 13")
	if err != nil {
		t.Fatal(err)
	}

	if !f.Eval(map[string]any{"f_int": 1, "f_uint": uint(13)}).Pass {
		t.Error("Packet must pass")
	}

	if f.Eval(map[string]any{"f_int": 1, "f_uint": uint(14)}).Pass {
		t.Error("Packet must not pass")
	}

	// field with multiple values
	f2, err := Parse("f_int != 2")
	if err != nil {
		t.Fatal(err)
	}
	if !f2.Eval(map[string]any{"f_int": []int{1, 3, 4}}).Pass {
		t.Error("Packet must pass")
	}

	if f2.Eval(map[string]any{"f_int": []int{1, 2, 3, 4}}).Pass {
		t.Error("Packet must not pass")
	}
}

func TestFilterMatchString(t *testing.T) {
	f, err := Parse("f_string.1 == \"1\" and f_string.2 == 47:45:54  and f_string.3 == \"abc123\"")
	if err != nil {
		t.Fatal(err)
	}
	if !f.Eval(map[string]any{"f_string.1": "1", "f_string.2": "GET", "f_string.3": "abc123"}).Pass {
		t.Error("Packet must pass")
	}

	if f.Eval(map[string]any{"f_string.1": "2", "f_string.2": "GET", "f_string.3": "abc123"}).Pass {
		t.Error("Packet must not pass")
	}

	f2, err := Parse("f_string.1 contains \"1\" and f_string.2 contains 47:45:54  and f_string.3 contains \"abc123\"")
	if err != nil {
		t.Fatal(err)
	}
	if !f2.Eval(map[string]any{"f_string.1": "asdf1asdf", "f_string.2": "text - GET ---", "f_string.3": "asf fffabc123"}).Pass {
		t.Error("Packet must pass")
	}

	if f2.Eval(map[string]any{"f_string.1": "test234test", "f_string.2": "xxxxETyyy", "f_string.3": "abc125"}).Pass {
		t.Error("Packet must not pass")
	}
}

func TestFilterMatchIP(t *testing.T) {
	f, err := Parse("ip.src==192.168.1.1 and ip.dst==192.168.1.1")
	require.NoError(t, err)

	cases := map[*map[string]any]TestResult{
		{
			"ip.src": net.ParseIP("192.168.1.1"),
			"ip.dst": net.ParseIP("192.168.1.1"),
		}: {
			Pass:          true,
			EvaluatedRule: "ip.src == 192.168.1.1 and ip.dst == 192.168.1.1",
		},
		{
			"ip.src": net.ParseIP("192.168.1.2"),
			"ip.dst": net.ParseIP("192.168.1.1"),
		}: {
			Pass:          false,
			EvaluatedRule: "ip.src == 192.168.1.1",
		},
		{}: {
			Pass:          false,
			MissingFields: []string{"ip.dst", "ip.src"},
			EvaluatedRule: "ip.src == 192.168.1.1 and ip.dst == 192.168.1.1",
		},
	}

	for input, want := range cases {
		got := toTestResult(f.Eval(*input))
		require.Equalf(t, want, got, "filter: %s, values: %+v", f.String(), input)
	}

	// CIDR test cases
	f4, err := Parse("ip.src == 192.168.0.0/16")
	require.NoError(t, err)

	cidrCases := map[*map[string]any]TestResult{
		{
			"ip.src": net.ParseIP("192.168.100.1"),
			"ip.dst": net.ParseIP("192.168.1.1"),
		}: {
			Pass:          true,
			EvaluatedRule: "ip.src == 192.168.0.0/16",
		},
		{
			"ip.src": net.ParseIP("172.16.0.1"),
			"ip.dst": net.ParseIP("10.0.0.1"),
		}: {
			Pass:          false,
			EvaluatedRule: "ip.src == 192.168.0.0/16",
		},
		{}: {
			Pass:          false,
			MissingFields: []string{"ip.src"},
			EvaluatedRule: "ip.src == 192.168.0.0/16",
		},
	}

	for input, want := range cidrCases {
		got := toTestResult(f4.Eval(*input))
		require.Equalf(t, want, got, "filter: %s, values: %+v", f4.String(), input)
	}
}

func TestFilterMatchMac(t *testing.T) {
	f, err := Parse("f_mac == ab:3b:06:07:b2:ef")
	require.NoError(t, err)

	h1, _ := net.ParseMAC("ab:3b:06:07:b2:ef")
	h2, _ := net.ParseMAC("aa:bb:cc:dd:ee:ff")

	cases := map[*map[string]any]TestResult{
		{
			"f_mac": h1,
		}: {
			Pass:          true,
			EvaluatedRule: "f_mac == ab:3b:06:07:b2:ef",
		},
		{
			"f_mac": h2,
		}: {
			Pass:          false,
			EvaluatedRule: "f_mac == ab:3b:06:07:b2:ef",
		},
		{}: {
			Pass:          false,
			MissingFields: []string{"f_mac"},
			EvaluatedRule: "f_mac == ab:3b:06:07:b2:ef",
		},
	}

	for input, want := range cases {
		got := toTestResult(f.Eval(*input))
		require.Equal(t, want, got)
	}
}

func TestParseError(t *testing.T) {
	// error
	for _, s := range []string{
		"??",
		"field == %1==",
		"== true",
		"test == >=",
		"field == 123 && ip == 1.2.3",
		"field == 123 && ip << 1",
		"str == 'bad qu\\\"ote'",
	} {
		_, err := Parse(s)
		if err == nil {
			t.Errorf("expected error for %s", s)
		}
	}

	// no error
	for _, s := range []string{
		"f_string == 123",
	} {
		_, err := Parse(s)
		if err != nil {
			t.Errorf("unexpected error for %s", s)
		}
	}
}

func FuzzParse(f *testing.F) {
	// Add initial corpus of valid and edge case inputs
	seeds := []string{
		"",
		"field == 1",
		"field.name == \"test\"",
		"field == 'test'",
		"field > 123",
		"field contains \"substring\"",
		"field matches /pattern/",
		"field == 192.168.1.1",
		"field == 01:02:03:04:05:06",
		"field == true",
		"not field",
		"field1 == 1 and field2 == 2",
		"field1 == 1 or field2 == 2",
		"(field1 == 1)",
		"field1 == 1 and (field2 == 2 or field3 == 3)",
		"field..name == 1",           // Invalid but shouldn't panic
		"field == \"unclosed string", // Invalid but shouldn't panic
		"field == 'unclosed string",  // Invalid but shouldn't panic
		"field == /unclosed regex",   // Invalid but shouldn't panic
		"field === value",            // Invalid operator but shouldn't panic
		"field == 192.168.1.256",     // Invalid IP but shouldn't panic
		"field == 01:ZZ:03",          // Invalid hex but shouldn't panic
	}

	for _, seed := range seeds {
		f.Add(seed)
	}

	f.Fuzz(func(t *testing.T, input string) {
		// Recover from any panics
		defer func() {
			if r := recover(); r != nil {
				t.Errorf("Parse panicked on input %q: %v", input, r)
			}
		}()

		// Call Parse and ignore the results
		_, _ = Parse(input)
	})
}

type testWriter struct {
	t *testing.T
}

func (w *testWriter) Write(p []byte) (n int, err error) {
	w.t.Helper()
	w.t.Log(strings.TrimRight(string(p), "\n"))
	return len(p), nil
}

func TestOptionalNegate(t *testing.T) {
	{
		f, err := Parse("field matches /pattern/")
		require.NoError(t, err)
		require.Equal(t, "field =~ /pattern/", f.String())

		assertEval(t, f, map[string]any{"field": "pattern"}, true)
		assertEval(t, f, map[string]any{"field": "other"}, false)

		r, ok := f.(*rule)
		require.True(t, ok)
		_, ok = r.Rule.(*nodeMatch)
		require.True(t, ok)
	}

	{
		f, err := Parse("field not matches /pattern/")
		require.NoError(t, err)
		require.Equal(t, "field not =~ /pattern/", f.String())

		assertEval(t, f, map[string]any{"field": "pattern"}, false)
		assertEval(t, f, map[string]any{"field": "other"}, true)

		r, ok := f.(*rule)
		require.True(t, ok)
		n, ok := r.Rule.(*nodeNot)
		require.True(t, ok)
		_, ok = n.right.(*nodeMatch)
		require.True(t, ok)
	}

	{
		f, err := Parse(`field !contains "str"`)
		require.NoError(t, err)
		require.Equal(t, `field not contains "str"`, f.String())

		assertEval(t, f, map[string]any{"field": "string"}, false)
		assertEval(t, f, map[string]any{"field": "other"}, true)

		r, ok := f.(*rule)
		require.True(t, ok)
		n, ok := r.Rule.(*nodeNot)
		require.True(t, ok)
		_, ok = n.right.(*nodeCompare)
		require.True(t, ok)
	}

	{
		f := MustParse(`field not in [1.1.1.1, 192.168.0.0/16]`)
		require.Equal(t, `field not in [1.1.1.1, 192.168.0.0/16]`, f.String())

		for ip, want := range map[string]bool{
			"1.1.1.1":     false,
			"192.168.0.0": false,
			"192.168.0.1": false,
			"192.0.1.1":   true,
			"1.1.1.2":     true,
		} {
			v := net.ParseIP(ip)
			assertEval(t, f, KV{"field": v}, want)
		}
	}
}

func TestArray(t *testing.T) {
	assertParseError(t, `field == [1,]`)
	assertParseError(t, `field == [1, [1, 2], 3]`)
	{
		f := MustParse(`field == [1, "str", 3]`)
		require.Equal(t, `field == [1, "str", 3]`, f.String())

		assertEval(t, f, KV{"field": 3}, true)
		assertEval(t, f, KV{"field": 4}, false)
		assertEval(t, f, KV{"field": "str"}, true)
	}

	{
		f := MustParse(`field contains [1, "str", 3]`)
		require.Equal(t, `field contains [1, "str", 3]`, f.String())

		// contains does not support arrays on the right side
		assertEval(t, f, KV{"field": "string"}, false)
		assertEval(t, f, KV{"field": "str"}, false)
		assertEval(t, f, KV{"field": 123}, false)
	}

	{
		f := MustParse(`field contains "str"`)
		require.Equal(t, `field contains "str"`, f.String())

		assertEval(t, f, KV{"field": "string"}, true) // substring
		assertEval(t, f, KV{"field": []any{"str", 123}}, true)
		assertEval(t, f, KV{"field": []any{"test", "string"}}, false)
	}

	assertParseEval(t, `f == "string"`, KV{"f": []any{1, "str", 3}}, false)       // false (no element equals "string")
	assertParseEval(t, `f != "string"`, KV{"f": []any{1, "str", 3}}, true)        // true  (no element equals "string")
	assertParseEval(t, `f contains "string"`, KV{"f": []any{1, "str", 3}}, false) // false (array doesn't contain "string")

	{
		f := MustParse(`arr contains val`)

		assertEval(t, f, KV{
			"arr": []any{1, "str", 3},
			"val": "str",
		}, true)
		assertEval(t, f, KV{
			"arr": []any{1, "str", 3},
			"val": 50,
		}, false)
	}

	assertParseEval(t, `[1,2,3] contains 2`, nil, true)
	assertParseEval(t, `[1,2,3] contains "str"`, nil, false)
}

func TestIn(t *testing.T) {
	{
		f := MustParse(`field in [1, "str", 3]`)
		require.Equal(t, `field in [1, "str", 3]`, f.String())

		assertEval(t, f, KV{"field": "string"}, false)
		assertEval(t, f, KV{"field": "str"}, true)
		assertEval(t, f, KV{"field": "s"}, false)
		assertEval(t, f, KV{"field": 123}, false)
	}

	assertParseEval(t, `5 in [1,2,3]`, nil, false)
	assertParseEval(t, `1.2.3.4 in [1.0.0.0/8, 8.8.8.8]`, nil, true)
	assertParseEval(t, `192.168.0.1 in [1.0.0.0/8, 8.8.8.8]`, nil, false)
}

func assertParseEval(t *testing.T, rule string, input KV, pass bool) {
	t.Helper()
	r := MustParse(rule)
	assertEval(t, r, input, pass)
}

// assertEval is a helper function to assert the result of a rule evaluation.
// It enforces strict evaluation.
func assertEval(t *testing.T, r Rule, input KV, pass bool) {
	t.Helper()
	res := r.Eval(input)

	var (
		ll   []string
		fail bool
	)
	if res.Pass != pass {
		fail = true
		ll = append(ll, fmt.Sprintf("⛑️  wanted [%t] got [%t]", pass, res.Pass))
	}
	if !res.Strict() {
		fail = true
		ll = append(ll, fmt.Sprintf("⛑️  missing fields: %v", res.MissingFields.Items()))
	}
	if fail {
		ll = append(ll, fmt.Sprintf("rule:\t%s", r.String()))
		ll = append(ll, fmt.Sprintf("eval:\t%s", res.EvaluatedRule.String()))
		ll = append(ll, fmt.Sprintf("input:\t%s", toJSON(t, input)))
		t.Errorf("%s\n%s", "FAIL", strings.Join(ll, "\n"))
	}
}

func assertParseError(t *testing.T, rule string) {
	_, err := Parse(rule)
	assert.Error(t, err)
}

func toJSON(t *testing.T, v any) string {
	t.Helper()
	b, err := json.Marshal(v)
	require.NoError(t, err)
	return string(b)
}

func TestOperationValidity(t *testing.T) {
	assertParseError(t, `f >= "string"`)
	assertParseError(t, `f < 1.2.3.4`)
	assertParseError(t, `f > 01:02:03:04:05:06`)
	assertParseError(t, `f <= true`)
	assertParseError(t, `f > /pattern/`)

	_ = MustParse(`f >= 1`)
	_ = MustParse(`f < 1.5`)
}

func TestSpecialBooleanFields(t *testing.T) {
	tests := []struct {
		name     string
		rule     string
		input    map[string]any
		expected bool
	}{
		{
			name:     "standalone true",
			rule:     "true",
			input:    map[string]any{},
			expected: true,
		},
		{
			name:     "standalone false",
			rule:     "false",
			input:    map[string]any{},
			expected: false,
		},
		{
			name:     "true equals true",
			rule:     "true == true",
			input:    map[string]any{},
			expected: true,
		},
		{
			name:     "false equals false",
			rule:     "false == false",
			input:    map[string]any{},
			expected: true,
		},
		{
			name:     "true not equals false",
			rule:     "true != false",
			input:    map[string]any{},
			expected: true,
		},
		{
			name:     "field equals true",
			rule:     "field == true",
			input:    map[string]any{"field": true},
			expected: true,
		},
		{
			name:     "field not equals true",
			rule:     "field != true",
			input:    map[string]any{"field": false},
			expected: true,
		},
		{
			name:     "case insensitive TRUE",
			rule:     "TRUE == true",
			input:    map[string]any{},
			expected: true,
		},
		{
			name:     "case insensitive FALSE",
			rule:     "FALSE == false",
			input:    map[string]any{},
			expected: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			rule := MustParse(tt.rule)
			result := rule.Eval(tt.input)
			if result.Pass != tt.expected {
				t.Errorf("Expected %v, got %v for rule: %s", tt.expected, result.Pass, tt.rule)
			}
		})
	}
}
