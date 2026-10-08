package rulekit

import (
	"context"
	"fmt"
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

func (r *ruleAssertion) Pass() *ruleAssertion {
	r.t.Helper()
	assert.True(r.t, r.result.Pass(), "expected rule to pass\n%s", r)
	return r
}

func (r *ruleAssertion) ErrorString(err string) *ruleAssertion {
	r.t.Helper()
	assert.EqualError(r.t, r.result.Error, err, "error should match\n%s", r)
	return r
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
	Functions FunctionSet
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

func mustMacroSet(t testing.TB, macros map[string]string) MacroSet {
	t.Helper()
	set := MacroSet{}
	for name, expr := range macros {
		require.NoError(t, set.Register(name, expr))
	}
	return set
}

func mustParseURL(t testing.TB, s string) URL {
	t.Helper()
	u, err := ParseURL(s)
	if err != nil {
		t.Fatal(err)
	}
	return u
}
