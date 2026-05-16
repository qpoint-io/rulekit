package rulekit

import (
	"context"
	"fmt"
	"net"
	"reflect"
	"slices"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

type ruleAssertion struct {
	t      *testing.T
	rule   Rule
	result Result
	input  Ctxer
}

func assertRulep(t *testing.T, rule string, input Ctxer) *ruleAssertion {
	t.Helper()
	r, err := Parse(rule)
	require.NoError(t, err)
	return assertRule(t, r, input)
}

func assertRule(t *testing.T, rule Rule, input Ctxer) *ruleAssertion {
	res := evalRule(rule, input)
	return &ruleAssertion{
		t:      t,
		rule:   rule,
		result: res,
		input:  input,
	}
}

func evalRule(rule Rule, input Ctxer) Result {
	ctx := context.Background()
	var ruleInput Input
	opts := Opts{}
	if input != nil {
		ctx, ruleInput, opts = input.EvalArgs()
	}
	return rule.Eval(ctx, ruleInput, opts)
}

func (r *ruleAssertion) String() string {
	return fmt.Sprintf("rule: %s\ninput: %+v\nerr: %+v\nval: %+v", r.rule, r.input, r.result.Error, r.result.Value)
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
	slices.Sort(fields)
	missing := append([]string(nil), r.result.MissingFields...)
	slices.Sort(missing)
	assert.Equal(r.t, fields, missing, "missing fields should match\n%s", r)
	return r
}

func (r *ruleAssertion) Error(err error) *ruleAssertion {
	r.t.Helper()
	assert.Equal(r.t, err, r.result.Error, "error should match\n%s", r)
	return r
}

func (r *ruleAssertion) ErrorString(err string) *ruleAssertion {
	r.t.Helper()
	assert.EqualError(r.t, r.result.Error, err, "error should match\n%s", r)
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

func parseCIDR(t *testing.T, s string) *net.IPNet {
	_, ipnet, err := net.ParseCIDR(s)
	require.NoError(t, err)
	return ipnet
}

type Ctxer interface {
	EvalArgs() (context.Context, Input, Opts)
}

type kv map[string]any

func (k kv) EvalArgs() (context.Context, Input, Opts) {
	return context.Background(), FromKV(KV(k)), Opts{}
}

type ctx struct {
	Context   context.Context
	Input     Input
	KV        KV
	Macros    MacroSet
	Functions map[string]*Function
	Trace     bool
}

func (c *ctx) EvalArgs() (context.Context, Input, Opts) {
	if c == nil {
		return context.Background(), nil, Opts{}
	}
	ctx := c.Context
	if ctx == nil {
		ctx = context.Background()
	}
	input := c.Input
	if input == nil && c.KV != nil {
		input = FromKV(c.KV)
	}
	return ctx, input, Opts{Trace: c.Trace, Macros: c.Macros, Functions: c.Functions}
}

func assertParseEval(t *testing.T, rule string, input Ctxer, pass bool) {
	t.Helper()
	r, err := Parse(rule)
	require.NoError(t, err)
	assertEval(t, r, input, pass)
}

// assertEval is a helper function to assert the result of a rule evaluation.
// It enforces strict evaluation.
func assertEval(t *testing.T, r Rule, input Ctxer, value any) {
	ctx := context.Background()
	var ruleInput Input
	opts := Opts{}
	if input != nil {
		ctx, ruleInput, opts = input.EvalArgs()
	}
	res := r.Eval(ctx, ruleInput, opts)
	if !res.Ok() {
		t.Errorf("rule.Eval(%v) failed: error=%v missing=%v", input, res.Error, res.MissingFields)
		return
	}
	if !reflect.DeepEqual(res.Value, value) {
		t.Errorf("rule.Eval(%v) = %v, want %v", input, res.Value, value)
	}
}

func assertParseError(t *testing.T, rule string) {
	_, err := Parse(rule)
	assert.Error(t, err)
}

func assertParseErrorValue(t *testing.T, rule string, expected string) {
	_, err := Parse(rule)
	assert.EqualError(t, err, expected)
}

func mustMacroSet(t testing.TB, macros map[string]string) MacroSet {
	t.Helper()
	set := MacroSet{}
	for name, expr := range macros {
		require.NoError(t, set.Register(name, expr))
	}
	return set
}

// TestResult mirrors Result for easier testing.
type TestResult struct {
	Value         any
	Error         error
	MissingFields []string
}

// toTestResult converts a Result to TestResult for easier test assertions
func toTestResult(r Result) TestResult {
	missing := append([]string(nil), r.MissingFields...)
	slices.Sort(missing)

	return TestResult{
		Value:         r.Value,
		Error:         r.Error,
		MissingFields: missing,
	}
}
