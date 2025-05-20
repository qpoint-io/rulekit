package rulekit

import (
	"errors"
	"net"
	"os"
	"reflect"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func init() {
	// SetDebugLevel(5)
	// SetErrorVerbose(true)
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

	assertRule(t, filter, kv{
		"tags":   []string{"db-svc", "internal-vlan", "unprivileged-user"},
		"domain": "example.com",
		"process": KV{
			"uid":  1000,
			"path": "/usr/bin/some-other-process",
		},
		"port": 8080,
	}).Ok().Value(true)

	assertRule(t, filter, kv{
		"destination": KV{
			"ip":   net.ParseIP("192.168.2.37"),
			"port": 22,
		},
	}).Ok().Pass()

	assertRule(t, filter, kv{
		"destination.ip":   net.ParseIP("1.1.1.1"),
		"destination.port": 22,
	}).NotOk().Value(nil)

	assertRule(t, filter, kv{
		"src.process.path": "/usr/bin/some-other-process",
	}).Ok().Pass()

	assertRule(t, filter, kv{
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
			got := toTestResult(p.Eval(&Ctx{KV: *input}))
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

	smallInput := &Ctx{KV: KV{"tags": "db-svc"}}
	largeInput := &Ctx{KV: KV{"tags": []string{"db-svc", "internal-vlan", "unprivileged-user"}, "domain": "example.com", "process.uid": 1000, "port": 8080, "destination.ip": net.ParseIP("192.168.2.37"), "destination.port": 8080}}

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

	if !f.Eval(&Ctx{KV: KV{"f_int": 1, "f_uint": uint(13)}}).Pass() {
		t.Error("Packet must pass")
	}

	if f.Eval(&Ctx{KV: KV{"f_int": 1, "f_uint": uint(14)}}).Pass() {
		t.Error("Packet must not pass")
	}

	// field with multiple values
	f2, err := Parse("f_int != 2")
	if err != nil {
		t.Fatal(err)
	}
	if !f2.Eval(&Ctx{KV: KV{"f_int": []int{1, 3, 4}}}).Pass() {
		t.Error("Packet must pass")
	}

	if f2.Eval(&Ctx{KV: KV{"f_int": []int{1, 2, 3, 4}}}).Pass() {
		t.Error("Packet must not pass")
	}
}

func TestFilterMatchString(t *testing.T) {
	f, err := Parse("f_string.1 == \"1\" and f_string.2 == 47:45:54  and f_string.3 == \"abc123\"")
	if err != nil {
		t.Fatal(err)
	}
	if !f.Eval(&Ctx{KV: KV{"f_string.1": "1", "f_string.2": "GET", "f_string.3": "abc123"}}).Pass() {
		t.Error("Packet must pass")
	}

	if f.Eval(&Ctx{KV: KV{"f_string.1": "2", "f_string.2": "GET", "f_string.3": "abc123"}}).Pass() {
		t.Error("Packet must not pass")
	}

	f2, err := Parse("f_string.1 contains \"1\" and f_string.2 contains 47:45:54  and f_string.3 contains \"abc123\"")
	if err != nil {
		t.Fatal(err)
	}
	if !f2.Eval(&Ctx{KV: KV{"f_string.1": "asdf1asdf", "f_string.2": "text - GET ---", "f_string.3": "asf fffabc123"}}).Pass() {
		t.Error("Packet must pass")
	}

	if f2.Eval(&Ctx{KV: KV{"f_string.1": "test234test", "f_string.2": "xxxxETyyy", "f_string.3": "abc125"}}).Pass() {
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
		got := toTestResult(f.Eval(&Ctx{KV: *input}))
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
		got := toTestResult(f4.Eval(&Ctx{KV: *input}))
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
		got := toTestResult(f.Eval(&Ctx{KV: *input}))
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

func TestArray(t *testing.T) {
	assertParseError(t, `field == [1,]`)           // trailing commas are not allowed
	assertParseError(t, `field == [1, [1, 2], 3]`) // nested arrays are not allowed

	assertRulep(t, `[1, "str", 3]`, nil).
		Ok().
		Value([]any{int64(1), "str", int64(3)}).
		EvaluatedRule(`[1, "str", 3]`)

	{
		f := MustParse(`field == [1, "str", 3]`)
		require.Equal(t, `field == [1, "str", 3]`, f.String())

		assertEval(t, f, kv{"field": 3}, true)
		assertEval(t, f, kv{"field": 4}, false)
		assertEval(t, f, kv{"field": "str"}, true)
	}

	{
		f := MustParse(`field contains [1, "str", 3]`)
		require.Equal(t, `field contains [1, "str", 3]`, f.String())

		// contains does not support arrays on the right side
		assertEval(t, f, kv{"field": "string"}, false)
		assertEval(t, f, kv{"field": "str"}, false)
		assertEval(t, f, kv{"field": 123}, false)
	}

	{
		f := MustParse(`field contains "str"`)
		require.Equal(t, `field contains "str"`, f.String())

		assertEval(t, f, kv{"field": "string"}, true) // substring
		assertEval(t, f, kv{"field": []any{"str", 123}}, true)
		assertEval(t, f, kv{"field": []any{"test", "string"}}, false)
	}

	assertParseEval(t, `f == "string"`, kv{"f": []any{1, "str", 3}}, false)       // false (no element equals "string")
	assertParseEval(t, `f != "string"`, kv{"f": []any{1, "str", 3}}, true)        // true  (no element equals "string")
	assertParseEval(t, `f contains "string"`, kv{"f": []any{1, "str", 3}}, false) // false (array doesn't contain "string")

	{
		f := MustParse(`arr contains val`)

		assertEval(t, f, kv{
			"arr": []any{1, "str", 3},
			"val": "str",
		}, true)
		assertEval(t, f, kv{
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

		assertEval(t, f, kv{"field": "string"}, false)
		assertEval(t, f, kv{"field": "str"}, true)
		assertEval(t, f, kv{"field": "s"}, false)
		assertEval(t, f, kv{"field": 123}, false)
	}

	assertParseEval(t, `5 in [1,2,3]`, nil, false)
	assertParseEval(t, `1.2.3.4 in [1.0.0.0/8, 8.8.8.8]`, nil, true)
	assertParseEval(t, `192.168.0.1 in [1.0.0.0/8, 8.8.8.8]`, nil, false)
	assertParseEval(t, `192.168.0.1 in 192.168.0.0/16`, nil, true)
	assertParseEval(t, `ip in 192.168.0.0/16`, kv{"ip": net.ParseIP("192.168.0.1")}, true)
	assertParseEval(t, `cidr contains ip`, kv{"cidr": parseCIDR(t, "192.168.0.0/16"), "ip": net.ParseIP("192.168.0.1")}, true)
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
			result := rule.Eval(&Ctx{KV: tt.input})
			if result.Value != tt.expected {
				t.Errorf("Expected %v, got %v for rule: %s", tt.expected, result.Value, tt.rule)
			}
		})
	}
}

func TestEvaluatedRule(t *testing.T) {
	rule := MustParse(`user == "root" or (dst.protocol == "mysql" and dst.port == 3306)`)
	// happy path
	assertRule(t, rule, kv{"user": "root"}).
		Ok().
		Pass().
		EvaluatedRule(`user == "root"`)
	assertRule(t, rule, kv{"user": "test", "dst.protocol": "mysql", "dst.port": 3306}).
		Ok().
		Pass().
		EvaluatedRule(`dst.protocol == "mysql" and dst.port == 3306`)
	assertRule(t, rule, kv{"user": "test", "dst.protocol": "mysql", "dst.port": 123}).
		Ok().
		Fail().
		EvaluatedRule(`user == "root" or dst.port == 3306`)

	// In the case of missing fields, EvaluatedRule should return an optimized rule that
	// allows us to progressively build up the rule's result.
	res1 := assertRule(t, rule, kv{}).
		MissingFields("user", "dst.protocol", "dst.port").
		Value(nil).
		EvaluatedRule(`user == "root" or (dst.protocol == "mysql" and dst.port == 3306)`).
		GetResult()

	res2 := assertRule(t, res1.EvaluatedRule, kv{"dst.protocol": "mysql"}).
		MissingFields("user", "dst.port").
		Value(nil).
		// we've only supplied dst.protocol, so the rule should be optimized
		EvaluatedRule(`user == "root" or dst.port == 3306`).
		GetResult()

	// dst.protocol should be "carried over" from the previous eval
	assertRule(t, res2.EvaluatedRule, kv{"dst.port": 3306}).
		Ok().
		// we have passed enough data for the and statement to pass
		Pass().
		EvaluatedRule(`dst.port == 3306`)
}

func TestStringAutomaticCasting(t *testing.T) {
	tests := []struct {
		name      string
		ruleExpr  string
		inputData map[string]any
		expected  bool
	}{
		{
			name:      "explicit IP equals string IP",
			ruleExpr:  `ip == "192.168.1.1"`,
			inputData: map[string]any{"ip": net.ParseIP("192.168.1.1")},
			expected:  true,
		},
		{
			name:      "string IP equals explicit IP",
			ruleExpr:  `"192.168.1.1" == ip`,
			inputData: map[string]any{"ip": net.ParseIP("192.168.1.1")},
			expected:  true,
		},
		{
			name:      "string field with IP value equals explicit IP",
			ruleExpr:  `ipstr == 192.168.1.1`,
			inputData: map[string]any{"ipstr": "192.168.1.1"},
			expected:  true,
		},
		{
			name:      "string IP in CIDR array",
			ruleExpr:  `"192.168.1.5" in [192.168.1.0/24]`,
			inputData: map[string]any{},
			expected:  true,
		},
		{
			name:      "string IP not in CIDR array",
			ruleExpr:  `"10.0.0.1" in [192.168.1.0/24]`,
			inputData: map[string]any{},
			expected:  false,
		},
		{
			name:      "string MAC equals MAC",
			ruleExpr:  `mac == "01:23:45:67:89:ab"`,
			inputData: map[string]any{"mac": mustParseMac("01:23:45:67:89:ab")},
			expected:  true,
		},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			assertRulep(t, tc.ruleExpr, kv(tc.inputData)).
				Ok().
				DoesPass(tc.expected)
		})
	}
}

func TestFunctionParsing(t *testing.T) {
	parsed := MustParse(`func_name(
		fieldarg,
		192.168.0.0,
		[1, 2, 3],
		nested_func(true)
	)`)

	fn := parsed.(*rule).Rule.(*FunctionValue)
	require.Equal(t, "func_name", fn.fn)
	require.Len(t, fn.args.vals, 4)
	assert.IsType(t, FieldValue(""), fn.args.vals[0])
	assert.IsType(t, &LiteralValue[any]{}, fn.args.vals[1])
	assert.IsType(t, net.IP{}, fn.args.vals[1].(*LiteralValue[any]).value)
	assert.IsType(t, &ArrayValue{}, fn.args.vals[2])
	assert.IsType(t, &FunctionValue{}, fn.args.vals[3])
}

func TestMacros(t *testing.T) {
	r := MustParse(`dst_k8s_svc() && user != "root"`)
	macros := map[string]Rule{
		"dst_k8s_svc": MustParse(`ip in 172.16.0.0/16 or host matches /svc.cluster.local$/`),
	}

	assertRule(t, r, &ctx{
		Macros: macros,
		KV: KV{
			"ip":   net.ParseIP("172.16.0.1"),
			"user": "nouser",
		},
	}).
		Pass().
		EvaluatedRule(`ip == 172.16.0.0/16 and user != "root"`)

	assertRule(t, r, &ctx{
		Macros: macros,
		KV: KV{
			"ip":   net.ParseIP("1.1.1.1"),
			"host": "1.1.1.1",
			"user": "nouser",
		},
	}).
		Fail().
		EvaluatedRule(`ip == 172.16.0.0/16 or host =~ /svc.cluster.local$/`)

	assertRule(t, r, &ctx{
		Macros: macros,
		KV: KV{
			"host": "test.svc.cluster.local",
			"user": "nouser",
		},
	}).
		Pass().
		EvaluatedRule(`host =~ /svc.cluster.local$/ and user != "root"`)
}

func TestCustomFunction(t *testing.T) {
	fns := map[string]*Function{
		"custom_func": {
			Args: []FunctionArg{
				{Name: "msg"},
			},
			Eval: func(args map[string]any) Result {
				msg, err := IndexFuncArg[string](args, "msg")
				if err != nil {
					return Result{Error: err}
				}
				return Result{Value: "Got msg: " + msg}
			},
		},
	}

	assertRulep(t, `custom_func("test")`, &ctx{
		Functions: fns,
	}).Ok().Value(`Got msg: test`)
	assertRulep(t, `custom_func(1.2.3.4)`, &ctx{
		Functions: fns,
	}).ErrorString(`arg msg: expected string, got net.IP`)
	assertRulep(t, `custom_func()`, &ctx{
		Functions: fns,
	}).ErrorString(`function "custom_func" expects 1 arguments, got 0`)
	assertRulep(t, `custom_func(1, 2)`, &ctx{
		Functions: fns,
	}).ErrorString(`function "custom_func" expects 1 arguments, got 2`)

	// mix & match functions, macros, stdlib functions
	assertRulep(t, `starts_with(macro(), "Got msg")`, &ctx{
		Functions: fns,
		Macros: map[string]Rule{
			"macro": MustParse(`custom_func("test")`),
		},
	}).Pass()
}

func TestCtx_Validate(t *testing.T) {
	tcs := []struct {
		name string
		ctx  *Ctx
		err  string
	}{
		{
			name: "happy path",
			ctx: &Ctx{
				Macros: map[string]Rule{
					"dst_k8s_svc": MustParse(`true`),
				},
				Functions: map[string]*Function{
					"custom_func": {},
				},
			},
		},
		{
			name: "nil func",
			ctx: &Ctx{
				Functions: map[string]*Function{
					"custom_func": nil,
				},
			},
			err: `function "custom_func": must not be nil`,
		},
		{
			name: "nil macro",
			ctx: &Ctx{
				Macros: map[string]Rule{
					"custom_macro": nil,
				},
			},
			err: `macro "custom_macro": must not be nil`,
		},
		{
			name: "macro name conflicts with function",
			ctx: &Ctx{
				Macros: map[string]Rule{
					"custom_func": MustParse(`true`),
				},
				Functions: map[string]*Function{
					"custom_func": {},
				},
			},
			err: `macro "custom_func": name conflicts with a custom function`,
		},
		{
			name: "macro name conflicts with stdlib function",
			ctx: &Ctx{
				Macros: map[string]Rule{
					"starts_with": MustParse(`true`),
				},
			},
			err: `macro "starts_with": name conflicts with a stdlib function`,
		},
		{
			name: "custom function name conflicts with stdlib function",
			ctx: &Ctx{
				Functions: map[string]*Function{
					"starts_with": {},
				},
			},
			err: `function "starts_with": name conflicts with a stdlib function`,
		},
	}

	for _, tc := range tcs {
		t.Run(tc.name, func(t *testing.T) {
			err := tc.ctx.Validate()
			if tc.err == "" {
				require.NoError(t, err)
			} else {
				require.EqualError(t, err, tc.err)
			}
		})
	}
}

func TestEval_invalid_ctx(t *testing.T) {
	assertRulep(t, `true`, &ctx{
		Functions: map[string]*Function{
			"starts_with": {},
		},
	}).Error(errors.New(`function "starts_with": name conflicts with a stdlib function`))
}

func TestFieldNames(t *testing.T) {
	valid := []string{
		"field",
		"field1",
		"field-",
		"field_",
		"field_1",
		"field-1",
		"field.1",
		"field-1.field2",
		"field-1.field2-3",
		"request.header.user-agent",
	}
	for _, field := range valid {
		r := field + ` == true`
		_, err := Parse(r)
		require.NoError(t, err, r)
	}

	invalid := []string{
		"-field",
		`field%.field`,
		"01fie^ld",
		"01fie||d",
	}
	for _, field := range invalid {
		r := field + ` == true`
		_, err := Parse(r)
		require.Error(t, err, r)
	}
}
