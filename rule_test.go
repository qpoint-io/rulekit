package rulekit

import (
	"net"
	"testing"

	"github.com/stretchr/testify/require"
)

// TestGoInputTypes covers Go input values that the JSON test vectors in
// testdata/vectors cannot express: typed slices, unsigned Go integers, and
// strings holding non-UTF-8 bytes. Their JSON-expressible equivalents are vectors.
func TestGoInputTypes(t *testing.T) {
	engine := `
		tags == 'db-svc'
		OR domain matches /example\.com$/ -- any domain or subdomain of example.com
		OR src.process.path matches |^/usr/bin/| -- patterns can be enclosed in |...| or /.../
		OR (process.uid != 0 AND tags contains 'internal-svc') 
		/* connections to LAN addresses over privileged ports */
		OR (destination.port <= 1023 AND destination.ip == 192.168.0.0/16)
	`
	tests := []struct {
		name  string
		rule  string
		input kv
		pass  bool
	}{
		{
			name: "[]string in engine example",
			rule: engine,
			input: kv{
				"tags":    []string{"db-svc", "internal-vlan", "unprivileged-user"},
				"domain":  "example.com",
				"process": KV{"uid": 1000, "path": "/usr/bin/some-other-process"},
				"port":    8080,
			},
			pass: true,
		},
		{"[]string == element", `domain == "example.com" AND tags == "db-svc"`, kv{"domain": "example.com", "tags": []string{"test", "db-svc"}}, true},
		{"empty []string == element", `domain == "example.com" AND tags == "db-svc"`, kv{"tags": []string{}}, false},
		{"uint equality", `f_int == 1 and f_uint == 13`, kv{"f_int": 1, "f_uint": uint(13)}, true},
		{"uint inequality", `f_int == 1 and f_uint == 13`, kv{"f_int": 1, "f_uint": uint(14)}, false},
		{"[]int != with no equal element", `f_int != 2`, kv{"f_int": []int{1, 3, 4}}, true},
		{"[]int != with an equal element", `f_int != 2`, kv{"f_int": []int{1, 2, 3, 4}}, false},
		{"[]string index", `items[1] == "second"`, kv{"items": []string{"first", "second"}}, true},
		{"[]string in array matches", `client in ["alice", "bob"]`, kv{"client": []string{"nobody", "bob"}}, true},
		{"[]string in array no match", `client in ["alice", "bob"]`, kv{"client": []string{"nobody", "somebody"}}, false},
		{"empty []string in array", `client in ["alice"]`, kv{"client": []string{}}, false},
		{"[]int in array matches", `ports in [80, 443]`, kv{"ports": []int{22, 443}}, true},
		{"[]int in array no match", `ports in [80, 443]`, kv{"ports": []int{22, 8080}}, false},
		{"[]int64 in array matches", `ports in [80, 443]`, kv{"ports": []int64{22, 80}}, true},
		{"[]net.IP in CIDR matches", `ips in 192.168.0.0/16`, kv{"ips": []net.IP{net.ParseIP("1.1.1.1"), net.ParseIP("192.168.0.1")}}, true},
		{"[]net.IP in CIDR no match", `ips in 192.168.0.0/16`, kv{"ips": []net.IP{net.ParseIP("1.1.1.1"), net.ParseIP("8.8.8.8")}}, false},
		{"[]net.IP in array matches", `ips in [1.0.0.0/8, 8.8.8.8]`, kv{"ips": []net.IP{net.ParseIP("192.168.0.1"), net.ParseIP("8.8.8.8")}}, true},
		{"not ([]string in array) when no element matches", `not (client in ["alice", "bob"])`, kv{"client": []string{"nobody", "somebody"}}, true},
		{"not ([]string in array) when an element matches", `not (client in ["alice", "bob"])`, kv{"client": []string{"nobody", "bob"}}, false},
		{"non-UTF-8 string equals nine hex pairs", `s == 01:23:45:67:89:ab:AB:cd:ef`, kv{"s": "\x01\x23\x45\x67\x89\xab\xab\xcd\xef"}, true},
		{"non-UTF-8 string equals two hex pairs", `x == ab:cd`, kv{"x": "\xab\xcd"}, true},
		{"non-UTF-8 string equals x-quoted hex", `x == x"0123456789abcdef"`, kv{"x": "\x01\x23\x45\x67\x89\xab\xcd\xef"}, true},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			assertRulep(t, tc.rule, tc.input).Ok().Value(tc.pass)
		})
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
	cases := []struct {
		name string
		expr string
		ctx  Ctxer
	}{
		{
			name: "short_circuit_first_branch_string",
			expr: `tags == "db-svc" or domain matches /example\.com$/ or destination.ip in 192.168.0.0/16`,
			ctx:  kv{"tags": "db-svc"},
		},
		{
			name: "short_circuit_first_branch_string_slice",
			expr: `tags == "db-svc" or domain matches /example\.com$/ or destination.ip in 192.168.0.0/16`,
			ctx:  kv{"tags": []string{"db-svc", "internal-vlan"}},
		},
		{
			name: "full_traversal_last_branch_pass",
			expr: `tags == "db-svc" or domain matches /example\.com$/ or process.uid == 0 or destination.ip in 192.168.0.0/16`,
			ctx: kv{
				"tags":        "other",
				"domain":      "qpoint.io",
				"process":     KV{"uid": 1000},
				"destination": KV{"ip": net.ParseIP("192.168.2.37")},
			},
		},
		{
			name: "full_traversal_no_match",
			expr: `tags == "db-svc" or domain matches /example\.com$/ or process.uid == 0 or destination.ip in 192.168.0.0/16`,
			ctx: kv{
				"tags":        "other",
				"domain":      "qpoint.io",
				"process":     KV{"uid": 1000},
				"destination": KV{"ip": net.ParseIP("10.0.0.1")},
			},
		},
		{
			name: "nested_path_number",
			expr: `process.uid != 0 and destination.port <= 1023`,
			ctx:  kv{"process": KV{"uid": 1000}, "destination": KV{"port": 443}},
		},
		{
			name: "bracket_path",
			expr: `request.headers["user-agent"] == "curl"`,
			ctx:  kv{"request": KV{"headers": KV{"user-agent": "curl"}}},
		},
		{
			name: "array_index_path",
			expr: `items[0].name == "first"`,
			ctx:  kv{"items": []any{KV{"name": "first"}, KV{"name": "second"}}},
		},
		{
			name: "regex",
			expr: `domain matches /example\.com$/`,
			ctx:  kv{"domain": "api.example.com"},
		},
		{
			name: "ip_cidr",
			expr: `destination.ip in 192.168.0.0/16`,
			ctx:  kv{"destination": KV{"ip": net.ParseIP("192.168.2.37")}},
		},
		{
			name: "missing_fields",
			expr: `user == "root" or destination.ip in 192.168.0.0/16`,
			ctx:  kv{},
		},
		{
			name: "function",
			expr: `starts_with(path, "/api")`,
			ctx:  kv{"path": "/api/v1"},
		},
		{
			name: "macro",
			expr: `is_internal() and user != "root"`,
			ctx: &ctx{
				KV:     KV{"ip": net.ParseIP("172.16.0.1"), "user": "api"},
				Macros: mustMacroSet(b, map[string]string{"is_internal": `ip in 172.16.0.0/16`}),
			},
		},
	}

	for _, tc := range cases {
		b.Run(tc.name, func(b *testing.B) {
			rule := MustParse(tc.expr)
			ctx, input, opts := tc.ctx.EvalArgs()
			b.ReportAllocs()
			b.ResetTimer()
			for range b.N {
				benchmarkResult = rule.Eval(ctx, input, opts)
			}
		})
	}
}

func BenchmarkEvalLazyInput(b *testing.B) {
	b.Run("pruned", func(b *testing.B) {
		rule := MustParse(`allow == true or expensive == "value"`)
		input := FromKV(KV{
			"allow": true,
			"expensive": LazyValue(func() (any, error) {
				return "value", nil
			}),
		})
		b.ReportAllocs()
		b.ResetTimer()
		for range b.N {
			benchmarkResult = rule.Eval(nil, input, Opts{})
		}
	})

	b.Run("resolved_cached", func(b *testing.B) {
		rule := MustParse(`expensive == "value"`)
		input := FromKV(KV{
			"expensive": LazyValue(func() (any, error) {
				return "value", nil
			}),
		})
		benchmarkResult = rule.Eval(nil, input, Opts{})
		b.ReportAllocs()
		b.ResetTimer()
		for range b.N {
			benchmarkResult = rule.Eval(nil, input, Opts{})
		}
	})

	b.Run("resolved_per_eval", func(b *testing.B) {
		rule := MustParse(`expensive == "value"`)
		b.ReportAllocs()
		b.ResetTimer()
		for range b.N {
			input := FromKV(KV{
				"expensive": LazyValue(func() (any, error) {
					return "value", nil
				}),
			})
			benchmarkResult = rule.Eval(nil, input, Opts{})
		}
	})
}

func BenchmarkEvalTrace(b *testing.B) {
	rule := MustParse(`tags == "db-svc" or domain matches /example\.com$/ or process.uid == 0 or destination.ip in 192.168.0.0/16`)
	input := FromKV(KV{
		"tags":        "other",
		"domain":      "qpoint.io",
		"process":     KV{"uid": 1000},
		"destination": KV{"ip": net.ParseIP("192.168.2.37")},
	})
	opts := Opts{Trace: true}
	b.ReportAllocs()
	b.ResetTimer()
	for range b.N {
		benchmarkResult = rule.Eval(nil, input, opts)
	}
}

var benchmarkResult Result

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

func TestMacroSetRegister(t *testing.T) {
	macros := MacroSet{}
	require.NoError(t, macros.Register("dst_k8s_svc", `ip in 172.16.0.0/16 or host matches /svc.cluster.local$/`))
	require.Equal(t, `ip in 172.16.0.0/16 or host matches /svc.cluster.local$/`, macros["dst_k8s_svc"].Source)
	require.NotNil(t, macros["dst_k8s_svc"].AST)
}

// TestFunctionErrorMessages pins Go error wording. Which expressions fail, and
// the parse error positions, are covered by testdata/vectors/functions.json.
func TestFunctionErrorMessages(t *testing.T) {
	assertRulep(t, "unknown_fn()", nil).ErrorString(`unknown function "unknown_fn"`)
	assertRulep(t, "unknown_fn(some_args)", nil).ErrorString(`unknown function "unknown_fn"`)

	for _, expr := range []string{"starts_with()", "starts_with(arg1)"} {
		_, err := Parse(expr)
		var parseErr *ParseError
		require.ErrorAs(t, err, &parseErr, expr)
		require.Contains(t, parseErr.Message, `function "starts_with" expects 2 arguments`, expr)
	}
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
		Macros:    mustMacroSet(t, map[string]string{"macro": `custom_func("test")`}),
	}).Pass()
}

func TestOpts_Validate(t *testing.T) {
	tcs := []struct {
		name string
		opts Opts
		err  string
	}{
		{
			name: "happy path",
			opts: Opts{
				Macros: mustMacroSet(t, map[string]string{"dst_k8s_svc": `true`}),
				Functions: map[string]*Function{
					"custom_func": {},
				},
			},
		},
		{
			name: "nil func",
			opts: Opts{
				Functions: map[string]*Function{
					"custom_func": nil,
				},
			},
			err: `function "custom_func": must not be nil`,
		},
		{
			name: "nil macro",
			opts: Opts{
				Macros: MacroSet{
					"custom_macro": nil,
				},
			},
			err: `macro "custom_macro": must not be nil`,
		},
		{
			name: "macro name conflicts with function",
			opts: Opts{
				Macros: mustMacroSet(t, map[string]string{"custom_func": `true`}),
				Functions: map[string]*Function{
					"custom_func": {},
				},
			},
			err: `macro "custom_func": name conflicts with a custom function`,
		},
		{
			name: "macro name conflicts with stdlib function",
			opts: Opts{
				Macros: mustMacroSet(t, map[string]string{"starts_with": `true`}),
			},
			err: `macro "starts_with": name conflicts with a stdlib function`,
		},
		{
			name: "custom function name conflicts with stdlib function",
			opts: Opts{
				Functions: map[string]*Function{
					"starts_with": {},
				},
			},
			err: `function "starts_with": name conflicts with a stdlib function`,
		},
	}

	for _, tc := range tcs {
		t.Run(tc.name, func(t *testing.T) {
			err := tc.opts.Validate()
			if tc.err == "" {
				require.NoError(t, err)
			} else {
				require.EqualError(t, err, tc.err)
			}
		})
	}
}
