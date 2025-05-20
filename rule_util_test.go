package rulekit

import (
	"encoding/json"
	"errors"
	"fmt"
	"net"
	"reflect"
	"slices"
	"strings"
	"testing"

	"github.com/hashicorp/go-multierror"
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
	ctx := &Ctx{}
	if input != nil {
		ctx = input.Ctx()
	}
	res := rule.Eval(ctx)
	return &ruleAssertion{
		t:      t,
		rule:   rule,
		result: res,
		input:  input,
	}
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

func (r *ruleAssertion) ErrorString(err string) *ruleAssertion {
	r.t.Helper()
	assert.EqualError(r.t, r.result.Error, err, "error should match\n%s", r)
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

func parseCIDR(t *testing.T, s string) *net.IPNet {
	_, ipnet, err := net.ParseCIDR(s)
	require.NoError(t, err)
	return ipnet
}

type Ctxer interface {
	Ctx() *Ctx
}

type kv map[string]any

func (k kv) Ctx() *Ctx {
	return &Ctx{KV: k}
}

type ctx Ctx

func (c *ctx) Ctx() *Ctx {
	return (*Ctx)(c)
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
	ctx := &Ctx{}
	if input != nil {
		ctx = input.Ctx()
	}
	res := r.Eval(ctx)
	if !res.Ok() {
		t.Errorf("rule.Eval(%v) failed: %v", input, res.Error)
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

func toJSON(t *testing.T, v any) string {
	t.Helper()
	b, err := json.Marshal(v)
	require.NoError(t, err)
	return string(b)
}

type testWriter struct {
	t *testing.T
}

func (w *testWriter) Write(p []byte) (n int, err error) {
	w.t.Helper()
	w.t.Log(strings.TrimRight(string(p), "\n"))
	return len(p), nil
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
