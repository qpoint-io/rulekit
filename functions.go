package rulekit

import (
	"context"
	"errors"
	"fmt"
	"net"
	"net/url"
	"reflect"
	"regexp"
	"strings"
	"unicode"
)

type FunctionValue struct {
	fn   string
	args *ArrayValue
}

func (f *FunctionValue) Eval(ctx context.Context, input Input, opts Opts) Result {
	if fn, ok := StdlibFuncs[f.fn]; ok {
		return f.eval(fn, ctx, input, opts)
	} else if fn, ok := opts.Functions[f.fn]; ok {
		return f.eval(fn, ctx, input, opts)
	} else if macro, ok := opts.Macros[f.fn]; ok {
		if len(f.args.vals) > 0 {
			return Result{
				Error: fmt.Errorf("macro %q expects 0 arguments, got %d", f.fn, len(f.args.vals)),
			}
		}
		res := macro.Rule.Eval(ctx, input, opts)
		if traceEnabled(opts) && res.Trace != nil {
			res.Trace = &Trace{
				Expr:          macro.Source,
				Value:         res.Value,
				Error:         res.Error,
				MissingFields: res.MissingFields,
				Status:        traceStatus(res),
				Active:        true,
				Children:      []*Trace{res.Trace},
			}
		}
		return res
	}

	return Result{
		Error: fmt.Errorf("unknown function %q", f.fn),
	}
}

// eval calls fn with the evaluated arguments.
func (f *FunctionValue) eval(fn *Function, ctx context.Context, input Input, opts Opts) Result {
	n := len(f.args.vals)
	if err := fn.checkArity(n); err != nil {
		return Result{Error: err}
	}

	// The arguments are evaluated into args, which is passed to the function
	// by value so that it stays on the stack.
	args := callArgs{n: n}
	var vals []any
	if n <= len(args.inline) {
		vals = args.inline[:n]
	} else {
		args.extra = make([]any, n)
		vals = args.extra
	}
	trace, res, ok := evalItems(ctx, input, opts, f.args.vals, vals)
	if !ok {
		return res
	}
	res = fn.call(ctx, args)
	if trace != nil {
		res.Trace = trace
	}
	return res
}

func (f *FunctionValue) String() string {
	return f.fn + "(" + f.args.String() + ")"
}

func (f *FunctionValue) Print(PrintMode) string {
	return f.String()
}

func newFunctionValue(fn string, args []Rule) *FunctionValue {
	argsArr := newArrayValue(args)
	argsArr.raw = strings.TrimPrefix(argsArr.raw, "[")
	argsArr.raw = strings.TrimSuffix(argsArr.raw, "]")
	return &FunctionValue{
		fn:   fn,
		args: argsArr,
	}
}

// FuncSchema names and documents a function defined with [Func].
type FuncSchema struct {
	// Name is the name rules call the function by.
	Name string
	// Doc describes the function, for tools and documentation.
	Doc string
}

// Function is a function callable from rules as name(arg, ...). Define one
// with [Func] and register it in [Opts.Functions] with [NewFunctionSet].
//
// The exported fields describe the function; changing them does not change
// how it is called.
type Function struct {
	// Name is the name rules call the function by, from [FuncSchema].
	Name string
	// Doc is the description from [FuncSchema].
	Doc string
	// Params are the parameters, in call order.
	Params []Param
	// Returns is the type name of the result; "any" when it is dynamic.
	Returns string

	// positional is the number of parameters, not counting a rest parameter.
	positional int
	rest       bool
	call       func(context.Context, callArgs) Result
}

// Param describes a parameter of a [Function].
type Param struct {
	// Name is the parameter name used in error messages.
	Name string
	// Type is the type name of the parameter: bool, int64, uint64, float64,
	// string, bytes, ip, cidr, mac, url, regex, array, object, or any.
	Type string
	// Rest is true for a [Rest] parameter, which takes the remaining
	// arguments.
	Rest bool
}

// Rest is the type of a rest parameter: the last field of a [Func]
// argument struct may have type Rest to take any number of remaining
// arguments, in call order (nil when there are none). The arguments are not
// converted. A field of type []any is an ordinary array parameter.
type Rest []any

// Func defines a function callable from rules, implemented by handler.
//
//	type clampArgs struct {
//		N   int64
//		Max int64 `rulekit:"max"`
//	}
//
//	var clamp = rulekit.Func(rulekit.FuncSchema{Name: "clamp", Doc: "Clamp n to max."},
//		func(ctx context.Context, a clampArgs) (int64, error) {
//			return min(a.N, a.Max), nil
//		})
//
// The fields of the argument struct A are the parameters, in order: a call
// passes one argument per field. A parameter is named after its field in
// snake_case (N is "n", MaxLen is "max_len", SrcIP is "src_ip"); a
// `rulekit:"name"` tag sets another name. Names appear in error messages
// and in [Function.Params].
//
// Each field has one of these types, and an argument converts to it only
// from a value of that type (an int64 parameter rejects a uint64 or float64
// argument):
//
//	bool                  bool
//	int64                 int64 (from int64 or int)
//	uint64                uint64 (from uint64 or uint)
//	float64               float64 (from float64 or float32)
//	string                string, including a URL's query
//	[]byte                bytes, including hex literals
//	net.IP                ip
//	*net.IPNet            cidr
//	net.HardwareAddr      mac
//	URL                   url (from URL or *url.URL)
//	*regexp.Regexp        regex
//	[]any                 array
//	map[string]any        object
//	any                   any value, unconverted (nil for null)
//
// A field may also have a named type defined over bool, int64, uint64,
// float64, string, []byte, []any, or map[string]any, such as
// `type Port uint64`; it takes the arguments its underlying type takes.
// (A type defined over net.IP or net.HardwareAddr has underlying type
// []byte, so it is a bytes parameter.) Named types over the struct and
// pointer types above are not supported.
//
// The last field may have type [Rest] to accept any number of further
// arguments.
//
// An argument of another type makes the rule result an
// [*ErrInvalidFunctionArg] error, and a call with the wrong number of
// arguments is an error; in both cases handler is not called. If an
// argument is missing from the input, the result is unknown and handler is
// not called.
//
// The result type R is any of the field types above other than Rest. A
// result of a named type is converted to its underlying type, so a Port
// result is a uint64 value. A handler error makes the rule result an error
// wrapping it; return [ErrMissing] to make the result unknown instead.
//
// ctx is the context passed to [Rule.Eval]. A function receives only its
// arguments, not the rule input: to apply logic to input values, pass them
// as arguments. (Macros, by contrast, see the input but take no
// arguments.)
//
// Arguments may share memory with the rule or its input: a handler must not
// modify them (for example, sorting a []any argument in place). Copy first,
// with [slices.Clone], to change one.
//
// Func panics if A is not a struct, has an unexported or embedded field, a
// field of an unsupported type, a Rest field that is not last, or two
// parameters with the same name, or if R is not supported, Name is empty,
// or handler is nil. Like [regexp.MustCompile], it is meant for
// package-level variables, so a bad definition fails at program start.
// Func uses reflection once, to build the function; calls do not.
func Func[A, R any](schema FuncSchema, handler func(context.Context, A) (R, error)) *Function {
	if schema.Name == "" {
		panic("rulekit.Func: FuncSchema.Name must not be empty")
	}
	fail := func(format string, args ...any) {
		panic(fmt.Sprintf("rulekit.Func(%q): ", schema.Name) + fmt.Sprintf(format, args...))
	}
	if handler == nil {
		fail("handler must not be nil")
	}
	spec, params := newArgSpec(reflect.TypeFor[A](), fail)
	rt := reflect.TypeFor[R]()
	returns, ok := kindOf(rt)
	if !ok || rt == reflect.TypeFor[Rest]() {
		fail("unsupported return type %s; supported: %s", rt, supportedTypes)
	}
	_, exact := valueKinds[rt]
	fn := &Function{
		Name:       schema.Name,
		Doc:        schema.Doc,
		Params:     params,
		Returns:    kindNames[returns],
		positional: len(spec.fields),
		rest:       spec.rest,
		call:       newCall(schema.Name, spec, handler, returns, exact),
	}
	return fn
}

func (fn *Function) checkArity(n int) error {
	if fn.rest {
		if n < fn.positional {
			return fmt.Errorf("function %q expects at least %d arguments, got %d", fn.Name, fn.positional, n)
		}
	} else if n != fn.positional {
		return fmt.Errorf("function %q expects %d arguments, got %d", fn.Name, fn.positional, n)
	}
	return nil
}

// FunctionSet holds functions by name, for [Opts.Functions].
type FunctionSet map[string]*Function

// NewFunctionSet returns a set of the functions, keyed by name. It panics
// if two have the same name or one is nil.
func NewFunctionSet(fns ...*Function) FunctionSet {
	set := make(FunctionSet, len(fns))
	for _, fn := range fns {
		if fn == nil {
			panic("rulekit.NewFunctionSet: nil function")
		}
		if _, ok := set[fn.Name]; ok {
			panic(fmt.Sprintf("rulekit.NewFunctionSet: duplicate function %q", fn.Name))
		}
		set[fn.Name] = fn
	}
	return set
}

// ErrMissing returns an error that, returned from a [Func] handler, makes
// the rule result unknown with the given MissingFields, like a field missing
// from the input. Use it when the function needs data that is not
// available yet. It may be wrapped. With no fields, the result is an error.
func ErrMissing(fields ...string) error {
	return &missingError{fields: fields}
}

type missingError struct {
	fields []string
}

func (e *missingError) Error() string {
	return "missing fields: " + strings.Join(e.fields, ", ")
}

// handlerResult is the result of a call whose handler returned err.
func handlerResult(name string, err error) Result {
	if missing, ok := errors.AsType[*missingError](err); ok && len(missing.fields) > 0 {
		return Result{MissingFields: missing.fields}
	}
	return Result{Error: fmt.Errorf("function %q: %w", name, err)}
}

// argKind is a parameter or result type of a [Func].
type argKind uint8

const (
	kindAny argKind = iota
	kindBool
	kindInt64
	kindUint64
	kindFloat64
	kindString
	kindBytes
	kindIP
	kindCIDR
	kindMAC
	kindURL
	kindRegex
	kindArray
	kindObject
)

var kindNames = [...]string{
	kindAny:     "any",
	kindBool:    "bool",
	kindInt64:   "int64",
	kindUint64:  "uint64",
	kindFloat64: "float64",
	kindString:  "string",
	kindBytes:   "bytes",
	kindIP:      "ip",
	kindCIDR:    "cidr",
	kindMAC:     "mac",
	kindURL:     "url",
	kindRegex:   "regex",
	kindArray:   "array",
	kindObject:  "object",
}

// valueKinds maps the Go types supported as parameters and results to their
// kinds.
var valueKinds = map[reflect.Type]argKind{
	reflect.TypeFor[any]():              kindAny,
	reflect.TypeFor[bool]():             kindBool,
	reflect.TypeFor[int64]():            kindInt64,
	reflect.TypeFor[uint64]():           kindUint64,
	reflect.TypeFor[float64]():          kindFloat64,
	reflect.TypeFor[string]():           kindString,
	reflect.TypeFor[[]byte]():           kindBytes,
	reflect.TypeFor[net.IP]():           kindIP,
	reflect.TypeFor[*net.IPNet]():       kindCIDR,
	reflect.TypeFor[net.HardwareAddr](): kindMAC,
	reflect.TypeFor[URL]():              kindURL,
	reflect.TypeFor[*regexp.Regexp]():   kindRegex,
	reflect.TypeFor[[]any]():            kindArray,
	reflect.TypeFor[map[string]any]():   kindObject,
}

const supportedTypes = "bool, int64, uint64, float64, string, []byte, net.IP, *net.IPNet, net.HardwareAddr, rulekit.URL, *regexp.Regexp, []any, map[string]any, any, or a named type over bool, int64, uint64, float64, string, []byte, []any, or map[string]any"

// kindOf returns the kind of a supported parameter or result type: one of
// valueKinds, or a named type whose underlying type is one of the
// non-struct, non-pointer types there. Values of such a named type have the
// same layout as its underlying type, so they are read and written as it.
func kindOf(t reflect.Type) (argKind, bool) {
	if kind, ok := valueKinds[t]; ok {
		return kind, true
	}
	switch t.Kind() {
	case reflect.Bool:
		return kindBool, true
	case reflect.Int64:
		return kindInt64, true
	case reflect.Uint64:
		return kindUint64, true
	case reflect.Float64:
		return kindFloat64, true
	case reflect.String:
		return kindString, true
	case reflect.Slice:
		switch t.Elem() {
		case reflect.TypeFor[byte]():
			return kindBytes, true
		case reflect.TypeFor[any]():
			return kindArray, true
		}
	case reflect.Map:
		if t.Key() == reflect.TypeFor[string]() && t.Elem() == reflect.TypeFor[any]() {
			return kindObject, true
		}
	}
	return 0, false
}

// argSpec says where each argument of a call is stored in the argument
// struct.
type argSpec struct {
	fields []argField
	// rest is set when the struct ends with a Rest field at restOffset.
	rest       bool
	restOffset uintptr
}

type argField struct {
	name   string
	offset uintptr
	kind   argKind
}

// newArgSpec builds the argSpec and Params for argument struct type t,
// calling fail for an unsupported struct.
func newArgSpec(t reflect.Type, fail func(string, ...any)) (*argSpec, []Param) {
	if t.Kind() != reflect.Struct {
		fail("argument type %s is not a struct", t)
	}
	spec := &argSpec{}
	params := make([]Param, 0, t.NumField())
	for i := range t.NumField() {
		f := t.Field(i)
		where := fmt.Sprintf("field %s of %s", f.Name, t)
		if f.Anonymous {
			fail("%s is embedded; declare each parameter as a named field", where)
		}
		if !f.IsExported() {
			fail("%s is unexported; every field is a parameter and must be exported", where)
		}
		name := f.Tag.Get("rulekit")
		if name == "" {
			name = snakeCase(f.Name)
		}
		for _, p := range params {
			if p.Name == name {
				fail("%s: duplicate parameter name %q", where, name)
			}
		}
		if f.Type == reflect.TypeFor[Rest]() {
			if i != t.NumField()-1 {
				fail("%s: a Rest field must be the last field", where)
			}
			spec.rest = true
			spec.restOffset = f.Offset
			params = append(params, Param{Name: name, Type: kindNames[kindAny], Rest: true})
			continue
		}
		kind, ok := kindOf(f.Type)
		if !ok {
			fail("%s has unsupported type %s; supported: %s, or rulekit.Rest as the last field", where, f.Type, supportedTypes)
		}
		spec.fields = append(spec.fields, argField{name: name, offset: f.Offset, kind: kind})
		params = append(params, Param{Name: name, Type: kindNames[kind]})
	}
	return spec, params
}

// snakeCase converts a Go field name to snake_case: an underscore goes
// before each upper-case letter that starts a word (after a lower-case
// letter or digit, or before a lower-case letter that ends an acronym), and
// letters are lower-cased. MaxLen is max_len, SrcIP is src_ip, HTTPCode is
// http_code.
func snakeCase(name string) string {
	runes := []rune(name)
	var b strings.Builder
	for i, r := range runes {
		if i > 0 && unicode.IsUpper(r) {
			prev := runes[i-1]
			nextLower := i+1 < len(runes) && unicode.IsLower(runes[i+1])
			if unicode.IsLower(prev) || unicode.IsDigit(prev) || (unicode.IsUpper(prev) && nextLower) {
				b.WriteByte('_')
			}
		}
		b.WriteRune(unicode.ToLower(r))
	}
	return b.String()
}

func (f *argField) invalid(value any) error {
	got := diagnosticType(value)
	if _, ok := value.(*url.URL); ok && f.kind == kindURL {
		got = "*url.URL that is not a valid rulekit URL"
	}
	return &ErrInvalidFunctionArg{Name: f.name, Expected: kindNames[f.kind], Got: got}
}

// Argument conversions. Each accepts exactly the Go types rulekit uses for
// one value type.

func asBool(v any) (bool, bool) {
	x, ok := v.(bool)
	return x, ok
}

func asInt64(v any) (int64, bool) {
	switch x := v.(type) {
	case int64:
		return x, true
	case int:
		return int64(x), true
	}
	return 0, false
}

func asUint64(v any) (uint64, bool) {
	switch x := v.(type) {
	case uint64:
		return x, true
	case uint:
		return uint64(x), true
	}
	return 0, false
}

func asFloat64(v any) (float64, bool) {
	switch x := v.(type) {
	case float64:
		return x, true
	case float32:
		return float64(x), true
	}
	return 0, false
}

func asString(v any) (string, bool) {
	switch x := v.(type) {
	case string:
		return x, true
	case urlQuery:
		return string(x), true
	}
	return "", false
}

func asBytes(v any) ([]byte, bool) {
	switch x := v.(type) {
	case []byte:
		return x, true
	case HexString:
		return x.Bytes, true
	}
	return nil, false
}

// asURL converts a URL, or a *url.URL by parsing its text form.
func asURL(v any) (URL, bool) {
	switch x := v.(type) {
	case URL:
		return x, true
	case *url.URL:
		if x == nil {
			return URL{}, false
		}
		u, err := parseURL(urlText(x))
		return u, err == nil
	}
	return URL{}, false
}

// asArray converts an array value. Typed slices from KV input are copied
// into a new []any.
func asArray(v any) ([]any, bool) {
	switch x := v.(type) {
	case []any:
		return x, true
	case []string:
		return boxSlice(x), true
	case []int:
		return boxSlice(x), true
	case []int64:
		return boxSlice(x), true
	case []uint:
		return boxSlice(x), true
	case []uint64:
		return boxSlice(x), true
	case []float32:
		return boxSlice(x), true
	case []float64:
		return boxSlice(x), true
	case []net.IP:
		return boxSlice(x), true
	}
	return nil, false
}

func boxSlice[T any](s []T) []any {
	out := make([]any, len(s))
	for i, v := range s {
		out[i] = v
	}
	return out
}
