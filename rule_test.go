package rulekit

import (
	"encoding/json"
	"errors"
	"fmt"
	"net"
	"os"
	"reflect"
	"slices"
	"strings"
	"testing"

	"github.com/hashicorp/go-multierror"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func init() {
	// SetDebugLevel(5)
	// SetErrorVerbose(true)
}

// TestResult mirrors Result but with EvaluatedRule as a string for easier testing
type TestResult struct {
	Value         any
	Error         error
	EvaluatedRule string // stores the string representation of the rule
}

type testErrMissingFields struct {
	fields []string
}

func (e *testErrMissingFields) Error() string {
	return fmt.Sprintf("missing fields: %v", e.fields)
}

func tErrMissingFields(fields ...string) error {
	return &testErrMissingFields{fields: fields}
}

// toTestResult converts a Result to TestResult for easier test assertions
func toTestResult(r Result) TestResult {
	err := r.Error
	if e, ok := err.(*multierror.Error); ok && e.Len() == 1 {
		err = e.Errors[0]
	}
	if e, ok := err.(*ErrMissingFields); ok {
		mf := e.Fields.Items()
		slices.Sort(mf)
		err = &testErrMissingFields{fields: mf}
	}

	tr := TestResult{
		Value: r.Value,
		Error: err,
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

	assertRule(t, filter, KV{
		"tags":   []string{"db-svc", "internal-vlan", "unprivileged-user"},
		"domain": "example.com",
		"process": KV{
			"uid":  1000,
			"path": "/usr/bin/some-other-process",
		},
		"port": 8080,
	}).Ok().Value(true)

	assertRule(t, filter, KV{
		"destination": KV{
			"ip":   net.ParseIP("192.168.2.37"),
			"port": 22,
		},
	}).Ok().Pass()

	assertRule(t, filter, KV{
		"destination.ip":   net.ParseIP("1.1.1.1"),
		"destination.port": 22,
	}).NotOk().Value(nil)

	assertRule(t, filter, KV{
		"src.process.path": "/usr/bin/some-other-process",
	}).Ok().Pass()

	assertRule(t, filter, KV{
		"src.process.path": "/opt/go",
	}).NotOk().Value(nil)
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
					Value:         true,
					EvaluatedRule: "tls_version == 1.2",
				},
				{"tls_version": 1.1}: {
					Value:         false,
					EvaluatedRule: "tls_version == 1.2",
				},
				{}: {
					Value:         nil,
					Error:         tErrMissingFields("tls_version"),
					EvaluatedRule: "tls_version == 1.2",
				},
			},
		},
		{
			filter: `tls_version != 5`,
			tests: map[*map[string]any]TestResult{
				{}: {
					Error:         tErrMissingFields("tls_version"),
					EvaluatedRule: "tls_version != 5",
				},
			},
		},
		{
			filter: `!tls_version`,
			tests: map[*map[string]any]TestResult{
				{}: {
					Error:         tErrMissingFields("tls_version"),
					EvaluatedRule: "!tls_version",
				},
			},
		},
		{
			filter: `domain matches /example\.com$/ OR tags == "db-svc"`,
			tests: map[*map[string]any]TestResult{
				{"domain": "example.com"}: {
					Value:         true,
					EvaluatedRule: `domain =~ /example\.com$/`,
				},
				{"tags": "db-svc"}: {
					Value:         true,
					EvaluatedRule: `tags == "db-svc"`,
				},
				{"domain": "other.com"}: {
					EvaluatedRule: `tags == "db-svc"`,
					Error:         tErrMissingFields("tags"),
				},
			},
		},
		{
			filter: `domain == "example.com" AND tags == "db-svc"`,
			tests: map[*map[string]any]TestResult{
				{"domain": "example.com"}: {
					Value:         nil,
					EvaluatedRule: `tags == "db-svc"`,
					Error:         tErrMissingFields("tags"),
				},
				{"tags": "db-svc"}: {
					Value:         nil,
					EvaluatedRule: `domain == "example.com"`,
					Error:         tErrMissingFields("domain"),
				},
				{"domain": "example.com", "tags": []string{"test", "db-svc"}}: {
					Value:         true,
					EvaluatedRule: `domain == "example.com" and tags == "db-svc"`,
				},
				{"domain": "qpoint.io"}: {
					Value:         false,
					EvaluatedRule: `domain == "example.com"`,
				},
				{"tags": []string{}}: {
					Value:         false,
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
					Value:         true,
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

				t.Errorf("Filter: %s\nInput: %+v\nGot:  %+v\nWant: %+v", tc.filter, *input, got, want)
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

	for expr, want := range map[string]any{
		"unset_field": nil,
		"zeroInt":     false,
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
		"zeroInt || zeroIP": false,
		"int && mac":        true,
	} {
		assertRulep(t, expr, values).Value(want)
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

	if !f.Eval(map[string]any{"f_int": 1, "f_uint": uint(13)}).Pass() {
		t.Error("Packet must pass")
	}

	if f.Eval(map[string]any{"f_int": 1, "f_uint": uint(14)}).Pass() {
		t.Error("Packet must not pass")
	}

	// field with multiple values
	f2, err := Parse("f_int != 2")
	if err != nil {
		t.Fatal(err)
	}
	if !f2.Eval(map[string]any{"f_int": []int{1, 3, 4}}).Pass() {
		t.Error("Packet must pass")
	}

	if f2.Eval(map[string]any{"f_int": []int{1, 2, 3, 4}}).Pass() {
		t.Error("Packet must not pass")
	}
}

func TestFilterMatchString(t *testing.T) {
	f, err := Parse("f_string.1 == \"1\" and f_string.2 == 47:45:54  and f_string.3 == \"abc123\"")
	if err != nil {
		t.Fatal(err)
	}
	if !f.Eval(map[string]any{"f_string.1": "1", "f_string.2": "GET", "f_string.3": "abc123"}).Pass() {
		t.Error("Packet must pass")
	}

	if f.Eval(map[string]any{"f_string.1": "2", "f_string.2": "GET", "f_string.3": "abc123"}).Pass() {
		t.Error("Packet must not pass")
	}

	f2, err := Parse("f_string.1 contains \"1\" and f_string.2 contains 47:45:54  and f_string.3 contains \"abc123\"")
	if err != nil {
		t.Fatal(err)
	}
	if !f2.Eval(map[string]any{"f_string.1": "asdf1asdf", "f_string.2": "text - GET ---", "f_string.3": "asf fffabc123"}).Pass() {
		t.Error("Packet must pass")
	}

	if f2.Eval(map[string]any{"f_string.1": "test234test", "f_string.2": "xxxxETyyy", "f_string.3": "abc125"}).Pass() {
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
			Value:         true,
			EvaluatedRule: "ip.src == 192.168.1.1 and ip.dst == 192.168.1.1",
		},
		{
			"ip.src": net.ParseIP("192.168.1.2"),
			"ip.dst": net.ParseIP("192.168.1.1"),
		}: {
			Value:         false,
			EvaluatedRule: "ip.src == 192.168.1.1",
		},
		{}: {
			Value:         nil,
			Error:         tErrMissingFields("ip.dst", "ip.src"),
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
			Value:         true,
			EvaluatedRule: "ip.src == 192.168.0.0/16",
		},
		{
			"ip.src": net.ParseIP("172.16.0.1"),
			"ip.dst": net.ParseIP("10.0.0.1"),
		}: {
			Value:         false,
			EvaluatedRule: "ip.src == 192.168.0.0/16",
		},
		{}: {
			Value:         nil,
			Error:         tErrMissingFields("ip.src"),
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
			Value:         true,
			EvaluatedRule: "f_mac == ab:3b:06:07:b2:ef",
		},
		{
			"f_mac": h2,
		}: {
			Value:         false,
			EvaluatedRule: "f_mac == ab:3b:06:07:b2:ef",
		},
		{}: {
			Value:         nil,
			Error:         tErrMissingFields("f_mac"),
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

func TestArray(t *testing.T) {
	assertParseError(t, `field == [1,]`)           // trailing commas are not allowed
	assertParseError(t, `field == [1, [1, 2], 3]`) // nested arrays are not allowed
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
	assertParseEval(t, `192.168.0.1 in 192.168.0.0/16`, nil, true)
	assertParseEval(t, `ip in 192.168.0.0/16`, KV{"ip": net.ParseIP("192.168.0.1")}, true)
	assertParseEval(t, `cidr contains ip`, KV{"cidr": parseCIDR(t, "192.168.0.0/16"), "ip": net.ParseIP("192.168.0.1")}, true)
}

func parseCIDR(t *testing.T, s string) *net.IPNet {
	_, ipnet, err := net.ParseCIDR(s)
	require.NoError(t, err)
	return ipnet
}

func assertParseEval(t *testing.T, rule string, input KV, pass bool) {
	t.Helper()
	r, err := Parse(rule)
	require.NoError(t, err)
	assertEval(t, r, input, pass)
}

// assertEval is a helper function to assert the result of a rule evaluation.
// It enforces strict evaluation.
func assertEval(t *testing.T, r Rule, input KV, value any) {
	t.Helper()
	res := r.Eval(input)

	var (
		ll   []string
		fail bool
	)
	if res.Value != value {
		fail = true
		ll = append(ll, fmt.Sprintf("⛑️  wanted [%t] got [%t]", value, res.Value))
	}
	if !res.Ok() {
		fail = true
		ll = append(ll, fmt.Sprintf("⛑️  err: %v", res.Error))
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
		expected any
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
			if result.Value != tt.expected {
				t.Errorf("Expected %v, got %v for rule: %s", tt.expected, result.Value, tt.rule)
			}
		})
	}
}

func TestEvaluatedRule(t *testing.T) {
	rule := MustParse(`user == "root" or (dst.protocol == "mysql" and dst.port == 3306)`)
	// happy path
	assertRule(t, rule, KV{"user": "root"}).
		Ok().
		Pass().
		EvaluatedRule(`user == "root"`)
	assertRule(t, rule, KV{"user": "test", "dst.protocol": "mysql", "dst.port": 3306}).
		Ok().
		Pass().
		EvaluatedRule(`dst.protocol == "mysql" and dst.port == 3306`)
	assertRule(t, rule, KV{"user": "test", "dst.protocol": "mysql", "dst.port": 123}).
		Ok().
		Fail().
		EvaluatedRule(`user == "root" or dst.port == 3306`)

	// In the case of missing fields, EvaluatedRule should return an optimized rule that
	// allows us to progressively build up the rule's result.
	res1 := assertRule(t, rule, KV{}).
		MissingFields("user", "dst.protocol", "dst.port").
		Value(nil).
		EvaluatedRule(`user == "root" or (dst.protocol == "mysql" and dst.port == 3306)`).
		GetResult()

	res2 := assertRule(t, res1.EvaluatedRule, KV{"dst.protocol": "mysql"}).
		MissingFields("user", "dst.port").
		Value(nil).
		// we've only supplied dst.protocol, so the rule should be optimized
		EvaluatedRule(`user == "root" or dst.port == 3306`).
		GetResult()

	// dst.protocol should be "carried over" from the previous eval
	assertRule(t, res2.EvaluatedRule, KV{"dst.port": 3306}).
		Ok().
		// we have passed enough data for the and statement to pass
		Pass().
		EvaluatedRule(`dst.port == 3306`)
}

type ruleAssertion struct {
	t      *testing.T
	rule   Rule
	result Result
	kv     KV
}

func assertRulep(t *testing.T, rule string, kv KV) *ruleAssertion {
	t.Helper()
	r, err := Parse(rule)
	require.NoError(t, err)
	return assertRule(t, r, kv)
}

func assertRule(t *testing.T, rule Rule, kv KV) *ruleAssertion {
	t.Helper()
	require.NotNil(t, rule, "rule cannot be nil")
	return &ruleAssertion{
		t:      t,
		rule:   rule,
		result: rule.Eval(kv),
		kv:     kv,
	}
}

func (r *ruleAssertion) String() string {
	return fmt.Sprintf("rule: %s\nkv: %+v\nerr: %+v\nval: %+v", r.rule, r.kv, r.result.Error, r.result.Value)
}

func (r *ruleAssertion) Value(value any) *ruleAssertion {
	r.t.Helper()
	assert.Equal(r.t, value, r.result.Value, "rule should return %+v\n%s", value, r)
	return r
}

func (r *ruleAssertion) Ok() *ruleAssertion {
	r.t.Helper()
	assert.True(r.t, r.result.Ok(), "rule should be ok\n%s", r)
	return r
}

func (r *ruleAssertion) DoesPass(pass bool) *ruleAssertion {
	r.t.Helper()
	switch pass {
	case true:
		assert.True(r.t, r.result.Pass(), "expected rule to pass\n%s", r)
	case false:
		assert.True(r.t, r.result.Fail(), "expected rule to fail\n%s", r)
	}
	return r
}

func (r *ruleAssertion) Pass() *ruleAssertion {
	r.t.Helper()
	assert.True(r.t, r.result.Pass(), "expected rule to pass\n%s", r)
	return r
}

func (r *ruleAssertion) Fail() *ruleAssertion {
	r.t.Helper()
	assert.True(r.t, r.result.Fail(), "expected rule to fail\n%s", r)
	return r
}

func (r *ruleAssertion) NotOk() *ruleAssertion {
	r.t.Helper()
	assert.False(r.t, r.result.Ok(), "rule should not be ok\n%s", r)
	return r
}

func (r *ruleAssertion) MissingFields(fields ...string) *ruleAssertion {
	r.t.Helper()
	var mf *ErrMissingFields
	if errors.As(r.result.Error, &mf) {
		slices.Sort(fields)
		missing := mf.Fields.Items()
		slices.Sort(missing)
		assert.Equal(r.t, fields, missing, "missing fields should match\n%s", r)
	} else {
		assert.Fail(r.t, "unexpected error type", "expected ErrMissingFields but got %T\n%s", r.result.Error, r)
	}
	return r
}

func (r *ruleAssertion) Error(err error) *ruleAssertion {
	r.t.Helper()
	assert.Equal(r.t, err, r.result.Error, "error should match\n%s", r)
	return r
}

func (r *ruleAssertion) EvaluatedRule(rule string) *ruleAssertion {
	r.t.Helper()
	assert.Equal(r.t, rule, r.result.EvaluatedRule.String(), "evaluated rule should match\n%s", r)
	return r
}

func (r *ruleAssertion) Result(expected TestResult) *ruleAssertion {
	r.t.Helper()
	assert.Equal(r.t, expected, toTestResult(r.result), "result should match\n%s", r)
	return r
}

func (r *ruleAssertion) GetResult() Result {
	return r.result
}
