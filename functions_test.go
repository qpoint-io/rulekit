package rulekit

import (
	"context"
	"errors"
	"net"
	"net/url"
	"regexp"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func testFunc(name string) *Function {
	return Func(FuncSchema{Name: name}, func(context.Context, struct{}) (bool, error) {
		return true, nil
	})
}

type clampArgs struct {
	N   int64
	Max int64 `rulekit:"limit"`
}

var testClamp = Func(FuncSchema{Name: "clamp", Doc: "Clamp n to limit."},
	func(_ context.Context, a clampArgs) (int64, error) {
		return min(a.N, a.Max), nil
	})

func TestFunc(t *testing.T) {
	fns := &ctx{Functions: NewFunctionSet(testClamp)}

	assertRulep(t, `clamp(150, 100) == 100`, fns).Pass()
	assertRulep(t, `clamp(5, 100)`, fns).Ok().Value(int64(5))

	assert.Equal(t, "clamp", testClamp.Name)
	assert.Equal(t, "Clamp n to limit.", testClamp.Doc)
	assert.Equal(t, []Param{{Name: "n", Type: "int64"}, {Name: "limit", Type: "int64"}}, testClamp.Params)
	assert.Equal(t, "int64", testClamp.Returns)
}

func TestFuncParamNames(t *testing.T) {
	type args struct {
		N        bool
		MaxLen   bool
		SrcIP    bool
		HTTPCode bool
		Renamed  bool `rulekit:"other"`
	}
	fn := Func(FuncSchema{Name: "f"}, func(context.Context, args) (bool, error) { return true, nil })
	var names []string
	for _, p := range fn.Params {
		names = append(names, p.Name)
	}
	assert.Equal(t, []string{"n", "max_len", "src_ip", "http_code", "other"}, names)
}

func TestFuncInvalidArg(t *testing.T) {
	fns := &ctx{Functions: NewFunctionSet(testClamp)}

	// An int64 parameter accepts neither a uint64 nor a string.
	for expr, got := range map[string]string{
		`clamp(1, 18446744073709551615)`: "uint64",
		`clamp(1, "2")`:                  "string",
		`clamp(1, 2.0)`:                  "float64",
	} {
		res := evalRule(MustParse(expr), fns)
		var argErr *ErrInvalidFunctionArg
		require.ErrorAs(t, res.Error, &argErr, expr)
		assert.Equal(t, ErrInvalidFunctionArg{Name: "limit", Expected: "int64", Got: got}, *argErr, expr)
	}
}

func TestFuncArity(t *testing.T) {
	fns := &ctx{Functions: NewFunctionSet(testClamp, testSum)}

	assertRulep(t, `clamp(1)`, fns).ErrorString(`function "clamp" expects 2 arguments, got 1`)
	assertRulep(t, `clamp(1, 2, 3)`, fns).ErrorString(`function "clamp" expects 2 arguments, got 3`)
	assertRulep(t, `sum()`, fns).ErrorString(`function "sum" expects at least 1 arguments, got 0`)
}

type sumArgs struct {
	First  int64
	Others Rest
}

var testSum = Func(FuncSchema{Name: "sum"}, func(_ context.Context, a sumArgs) (int64, error) {
	total := a.First
	for _, v := range a.Others {
		n, ok := v.(int64)
		if !ok {
			return 0, errors.New("not an int64")
		}
		total += n
	}
	return total, nil
})

func TestFuncRest(t *testing.T) {
	fns := &ctx{Functions: NewFunctionSet(testSum)}

	assertRulep(t, `sum(1)`, fns).Ok().Value(int64(1))
	assertRulep(t, `sum(1, 2, 3)`, fns).Ok().Value(int64(6))
	assertRulep(t, `sum(1, 2, 3, 4, 5, 6)`, fns).Ok().Value(int64(21))
	assert.Equal(t, Param{Name: "others", Type: "any", Rest: true}, testSum.Params[1])

	// Many arguments, with no rest field.
	type six struct{ A, B, C, D, E, F string }
	cat := Func(FuncSchema{Name: "cat"}, func(_ context.Context, a six) (string, error) {
		return a.A + a.B + a.C + a.D + a.E + a.F, nil
	})
	assertRulep(t, `cat("a", "b", "c", "d", "e", "f")`, &ctx{Functions: NewFunctionSet(cat)}).Ok().Value("abcdef")
}

func TestFuncContext(t *testing.T) {
	type key struct{}
	tenant := Func(FuncSchema{Name: "tenant"}, func(ctx context.Context, _ struct{}) (any, error) {
		return ctx.Value(key{}), nil
	})
	assertRulep(t, `tenant() == "acme"`, &ctx{
		Context:   context.WithValue(context.Background(), key{}, "acme"),
		Functions: NewFunctionSet(tenant),
	}).Pass()
}

func TestFuncErrMissing(t *testing.T) {
	type args struct{ Field string }
	lookup := Func(FuncSchema{Name: "lookup"}, func(_ context.Context, a args) (bool, error) {
		switch a.Field {
		case "wrapped":
			return false, errors.Join(errors.New("context"), ErrMissing("user.name"))
		case "none":
			return false, ErrMissing()
		}
		return false, ErrMissing(a.Field, "other")
	})
	fns := &ctx{Functions: NewFunctionSet(lookup)}

	res := evalRule(MustParse(`lookup("user.id")`), fns)
	assert.True(t, res.Unknown())
	assert.Equal(t, []string{"user.id", "other"}, res.MissingFields)

	res = evalRule(MustParse(`lookup("wrapped")`), fns)
	assert.True(t, res.Unknown())
	assert.Equal(t, []string{"user.name"}, res.MissingFields)

	res = evalRule(MustParse(`lookup("none")`), fns)
	assert.Error(t, res.Error)

	// A missing field joins the function's missing fields like any other.
	res = evalRule(MustParse(`lookup("user.id") or flag`), fns)
	assert.ElementsMatch(t, []string{"user.id", "other", "flag"}, res.MissingFields)
}

func TestFuncHandlerError(t *testing.T) {
	sentinel := errors.New("boom")
	fail := Func(FuncSchema{Name: "fail"}, func(context.Context, struct{}) (bool, error) {
		return false, sentinel
	})
	res := evalRule(MustParse(`fail()`), &ctx{Functions: NewFunctionSet(fail)})
	assert.ErrorIs(t, res.Error, sentinel)
	assert.EqualError(t, res.Error, `function "fail": boom`)
}

func TestFuncMissingArgument(t *testing.T) {
	called := false
	type args struct{ V any }
	fn := Func(FuncSchema{Name: "f"}, func(_ context.Context, _ args) (bool, error) {
		called = true
		return true, nil
	})
	res := evalRule(MustParse(`f(absent)`), &ctx{KV: KV{}, Functions: NewFunctionSet(fn)})
	assert.True(t, res.Unknown())
	assert.Equal(t, []string{"absent"}, res.MissingFields)
	assert.False(t, called)
}

// identity is a function returning its one argument, of type T.
func identity[T any]() *Function {
	type args struct{ V T }
	return Func(FuncSchema{Name: "id"}, func(_ context.Context, a args) (T, error) {
		return a.V, nil
	})
}

func TestFuncTypes(t *testing.T) {
	ip := net.ParseIP("10.0.0.1")
	_, cidr, _ := net.ParseCIDR("10.0.0.0/8")
	mac, _ := parseMAC("01:23:45:67:89:ab")
	u := mustParseURL(t, "https://Example.com/a?b=c")
	stdURL, _ := url.Parse("https://Example.com/a?b=c")
	re := regexp.MustCompile("x")

	tcs := []struct {
		name string
		fn   *Function
		in   any
		want any
	}{
		{"any", identity[any](), int8(1), int8(1)},
		{"any null", identity[any](), nil, nil},
		{"bool", identity[bool](), true, true},
		{"int64", identity[int64](), int64(-1), int64(-1)},
		{"int64 from int", identity[int64](), 1, int64(1)},
		{"uint64", identity[uint64](), uint64(1), uint64(1)},
		{"uint64 from uint", identity[uint64](), uint(1), uint64(1)},
		{"float64", identity[float64](), 1.5, 1.5},
		{"float64 from float32", identity[float64](), float32(1.5), 1.5},
		{"string", identity[string](), "s", "s"},
		{"string from url query", identity[string](), urlQuery("b=c"), "b=c"},
		{"bytes", identity[[]byte](), []byte("ab"), []byte("ab")},
		{"bytes from hex", identity[[]byte](), HexString{Bytes: []byte("ab")}, []byte("ab")},
		{"ip", identity[net.IP](), ip, ip},
		{"cidr", identity[*net.IPNet](), cidr, cidr},
		{"mac", identity[net.HardwareAddr](), mac, mac},
		{"url", identity[URL](), u, u},
		{"url from *url.URL", identity[URL](), stdURL, u},
		{"regex", identity[*regexp.Regexp](), re, re},
		{"array", identity[[]any](), []any{"a", int64(1)}, []any{"a", int64(1)}},
		{"array from []string", identity[[]any](), []string{"a"}, []any{"a"}},
		{"object", identity[map[string]any](), KV{"a": "b"}, KV{"a": "b"}},
	}
	for _, tc := range tcs {
		t.Run(tc.name, func(t *testing.T) {
			res := evalRule(MustParse(`id(v)`), &ctx{KV: KV{"v": tc.in}, Functions: NewFunctionSet(tc.fn)})
			require.NoError(t, res.Error)
			assert.Equal(t, tc.want, res.Value)
		})
	}

	// Conversions are exact.
	for name, tc := range map[string]struct {
		fn *Function
		in any
	}{
		"int64 from uint64": {identity[int64](), uint64(1)},
		"uint64 from int64": {identity[uint64](), int64(1)},
		"float64 from int":  {identity[float64](), 1},
		"string from ip":    {identity[string](), ip},
		"ip from string":    {identity[net.IP](), "10.0.0.1"},
		"bool from null":    {identity[bool](), nil},
		"array from object": {identity[[]any](), KV{}},
	} {
		res := evalRule(MustParse(`id(v)`), &ctx{KV: KV{"v": tc.in}, Functions: NewFunctionSet(tc.fn)})
		var argErr *ErrInvalidFunctionArg
		assert.ErrorAs(t, res.Error, &argErr, name)
	}
}

func TestFuncPanics(t *testing.T) {
	type embedded struct{ A int64 }
	ok := func(context.Context, struct{ A int64 }) (bool, error) { return true, nil }

	tcs := []struct {
		name string
		// field is a name the panic message must mention, besides the function.
		field  string
		define func()
	}{
		{"argument not a struct", "int64", func() {
			Func(FuncSchema{Name: "fn"}, func(context.Context, int64) (bool, error) { return true, nil })
		}},
		{"unexported field", "lower", func() {
			type args struct{ lower int64 }
			Func(FuncSchema{Name: "fn"}, func(context.Context, args) (bool, error) { return true, nil })
		}},
		{"embedded field", "embedded", func() {
			type args struct{ embedded }
			Func(FuncSchema{Name: "fn"}, func(context.Context, args) (bool, error) { return true, nil })
		}},
		{"unsupported field type", "Count", func() {
			type args struct{ Count int }
			Func(FuncSchema{Name: "fn"}, func(context.Context, args) (bool, error) { return true, nil })
		}},
		{"rest not last", "More", func() {
			type args struct {
				More Rest
				A    int64
			}
			Func(FuncSchema{Name: "fn"}, func(context.Context, args) (bool, error) { return true, nil })
		}},
		{"duplicate parameter name", "B", func() {
			type args struct {
				A int64
				B int64 `rulekit:"a"`
			}
			Func(FuncSchema{Name: "fn"}, func(context.Context, args) (bool, error) { return true, nil })
		}},
		{"unsupported return type", "int32", func() {
			Func(FuncSchema{Name: "fn"}, func(context.Context, struct{}) (int32, error) { return 0, nil })
		}},
		{"rest return type", "Rest", func() {
			Func(FuncSchema{Name: "fn"}, func(context.Context, struct{}) (Rest, error) { return nil, nil })
		}},
		{"nil handler", "", func() {
			Func[struct{}, bool](FuncSchema{Name: "fn"}, nil)
		}},
		{"empty name", "", func() {
			Func(FuncSchema{}, ok)
		}},
	}
	for _, tc := range tcs {
		t.Run(tc.name, func(t *testing.T) {
			defer func() {
				r := recover()
				require.NotNil(t, r, "Func should panic")
				msg, _ := r.(string)
				assert.Contains(t, msg, "rulekit.Func")
				if tc.name != "empty name" {
					assert.Contains(t, msg, `"fn"`)
				}
				assert.Contains(t, msg, tc.field)
			}()
			tc.define()
		})
	}
}

func TestNewFunctionSetDuplicate(t *testing.T) {
	assert.Panics(t, func() { NewFunctionSet(testFunc("a"), testFunc("a")) })
}

// TestFunctionsWithMacros mixes custom functions, macros, and stdlib
// functions.
func TestFunctionsWithMacros(t *testing.T) {
	type args struct{ Msg string }
	greet := Func(FuncSchema{Name: "greet"}, func(_ context.Context, a args) (string, error) {
		return "Got msg: " + a.Msg, nil
	})
	assertRulep(t, `starts_with(macro(), "Got msg")`, &ctx{
		Functions: NewFunctionSet(greet),
		Macros:    mustMacroSet(t, map[string]string{"macro": `greet("test")`}),
	}).Pass()
}
