package rulekit

import (
	"bytes"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"net"
	"net/url"
	"os"
	"path/filepath"
	"reflect"
	"slices"
	"strconv"
	"strings"
	"testing"

	"github.com/stretchr/testify/require"
)

// The language-neutral test vectors in testdata/vectors are shared with other
// implementations; testdata/vectors/README.md documents the format.

const vectorDir = "testdata/vectors"

type vectorFile struct {
	InputMode string       `json:"input_mode"`
	Cases     []vectorCase `json:"cases"`
}

type vectorCase struct {
	Name      string            `json:"name"`
	Expr      *string           `json:"expr"`
	InputMode string            `json:"input_mode"`
	Input     json.RawMessage   `json:"input"`
	Macros    map[string]string `json:"macros"`
	Expect    vectorExpect      `json:"expect"`
}

type vectorExpect struct {
	DecodeError   bool              `json:"decode_error"`
	ParseError    *vectorParseError `json:"parse_error"`
	Value         json.RawMessage   `json:"value"`
	Error         *bool             `json:"error"`
	MissingFields *[]string         `json:"missing_fields"`
	Trace         *vectorTrace      `json:"trace"`
	Print         *vectorPrint      `json:"print"`
	ASTJSON       json.RawMessage   `json:"ast_json"`
}

func (e vectorExpect) evaluates() bool {
	return e.Value != nil || e.Error != nil || e.MissingFields != nil || e.Trace != nil
}

// vectorParseError is either a bool or a {line, column} position.
type vectorParseError struct {
	Fails  bool
	Line   int
	Column int
}

func (p *vectorParseError) UnmarshalJSON(data []byte) error {
	if err := json.Unmarshal(data, &p.Fails); err == nil {
		return nil
	}
	var pos struct {
		Line   int `json:"line"`
		Column int `json:"column"`
	}
	dec := json.NewDecoder(bytes.NewReader(data))
	dec.DisallowUnknownFields()
	if err := dec.Decode(&pos); err != nil {
		return fmt.Errorf("parse_error must be a bool or {line, column}: %w", err)
	}
	if pos.Line < 1 || pos.Column < 1 {
		return fmt.Errorf("parse_error position must have line and column >= 1")
	}
	p.Fails, p.Line, p.Column = true, pos.Line, pos.Column
	return nil
}

type vectorPrint struct {
	String       *string `json:"string"`
	Compact      *string `json:"compact"`
	Source       *string `json:"source"`
	Multiline2sp *string `json:"multiline_2sp"`
	Multiline4sp *string `json:"multiline_4sp"`
}

type vectorTrace struct {
	Kind          *string             `json:"kind"`
	Expr          *string             `json:"expr"`
	Status        *string             `json:"status"`
	Value         json.RawMessage     `json:"value"`
	Active        *bool               `json:"active"`
	Pruned        *bool               `json:"pruned"`
	MissingFields *[]string           `json:"missing_fields"`
	Diagnostics   *[]vectorDiagnostic `json:"diagnostics"`
	Children      *[]vectorTrace      `json:"children"`
}

type vectorDiagnostic struct {
	Code      string `json:"code"`
	LeftType  string `json:"left_type"`
	Operator  string `json:"operator"`
	RightType string `json:"right_type"`
}

func TestVectors(t *testing.T) {
	entries, err := os.ReadDir(vectorDir)
	require.NoError(t, err)

	var files []string
	for _, entry := range entries {
		name := entry.Name()
		switch {
		case name == "README.md":
		case !entry.IsDir() && strings.HasSuffix(name, ".json"):
			files = append(files, name)
		default:
			t.Fatalf("unexpected entry %q in %s", name, vectorDir)
		}
	}
	require.NotEmpty(t, files, "no vector files found")

	for _, name := range files {
		t.Run(strings.TrimSuffix(name, ".json"), func(t *testing.T) {
			file := loadVectorFile(t, filepath.Join(vectorDir, name))
			require.NotEmpty(t, file.Cases, "vector file has no cases")
			seen := map[string]bool{}
			for _, tc := range file.Cases {
				require.NotEmpty(t, tc.Name, "case without name")
				require.False(t, seen[tc.Name], "duplicate case name %q", tc.Name)
				seen[tc.Name] = true
				mode := tc.InputMode
				if mode == "" {
					mode = file.InputMode
				}
				t.Run(tc.Name, func(t *testing.T) { runVectorCase(t, tc, mode) })
			}
		})
	}
}

func loadVectorFile(t *testing.T, path string) vectorFile {
	t.Helper()
	data, err := os.ReadFile(path)
	require.NoError(t, err)
	dec := json.NewDecoder(bytes.NewReader(data))
	dec.DisallowUnknownFields()
	var file vectorFile
	require.NoError(t, dec.Decode(&file), path)
	require.False(t, dec.More(), "%s: trailing data", path)
	return file
}

func vectorJSONOptions(t *testing.T, mode string) JSONOptions {
	t.Helper()
	switch mode {
	case "plain":
		return JSONOptions{}
	case "annotated_keys":
		return JSONOptions{AnnotatedKeys: true}
	case "typed_document":
		return JSONOptions{TypedDocument: true}
	default:
		t.Fatalf("input_mode must be plain, annotated_keys, or typed_document; got %q", mode)
		return JSONOptions{}
	}
}

func runVectorCase(t *testing.T, tc vectorCase, inputMode string) {
	want := tc.Expect
	require.True(t, want.DecodeError || want.ParseError != nil || want.evaluates() || want.Print != nil || want.ASTJSON != nil,
		"case has no expectations")

	var input Input
	if tc.Input != nil {
		kv, err := DecodeJSON(tc.Input, vectorJSONOptions(t, inputMode))
		if want.DecodeError {
			require.Error(t, err, "expected input decode error")
			require.Nil(t, tc.Expr, "decode_error cases must not have an expr")
			return
		}
		require.NoError(t, err, "decoding input")
		input = FromKV(kv)
	} else {
		require.False(t, want.DecodeError, "decode_error requires input")
	}

	require.NotNil(t, tc.Expr, "case requires expr")
	expr := *tc.Expr

	ast, err := ParseAST(expr)
	var rule Rule
	if err == nil {
		rule, err = Compile(ast)
	}
	if want.ParseError != nil && want.ParseError.Fails {
		require.Error(t, err, "expected parse error")
		require.False(t, want.evaluates() || want.Print != nil || want.ASTJSON != nil, "parse_error cases have no other expectations")
		if want.ParseError.Line > 0 {
			var parseErr *ParseError
			require.ErrorAs(t, err, &parseErr)
			require.Equal(t, want.ParseError.Line, parseErr.Line, "parse error line")
			require.Equal(t, want.ParseError.Column, parseErr.Column, "parse error column")
		}
		return
	}
	require.NoError(t, err, "parse")

	if want.Print != nil {
		checkVectorPrint(t, ast, rule, *want.Print)
	}
	if want.ASTJSON != nil {
		got, err := json.Marshal(ast)
		require.NoError(t, err)
		require.JSONEq(t, string(want.ASTJSON), string(got))
	}
	if !want.evaluates() {
		return
	}

	var macros MacroSet
	for name, source := range tc.Macros {
		require.NoError(t, macros.Register(name, source), "macro %q", name)
	}
	opts := Opts{Macros: macros}

	plain := rule.Eval(nil, input, opts)
	require.Nil(t, plain.Trace, "trace must be nil when tracing is off")
	checkVectorResult(t, plain, want)

	opts.Trace = true
	traced := rule.Eval(nil, input, opts)
	checkVectorResult(t, traced, want)
	if want.Trace != nil {
		require.NotNil(t, traced.Trace, "trace")
		checkVectorTrace(t, "trace", traced.Trace, *want.Trace)
	}
}

func checkVectorResult(t *testing.T, got Result, want vectorExpect) {
	t.Helper()
	wantErr := want.Error != nil && *want.Error
	if wantErr {
		require.Error(t, got.Error, "expected eval error")
	} else {
		require.NoError(t, got.Error, "eval error")
	}
	var wantMissing []string
	if want.MissingFields != nil {
		wantMissing = *want.MissingFields
	}
	require.Equal(t, sortedStrings(wantMissing), sortedStrings(got.MissingFields), "missing_fields")
	if want.Value != nil {
		requireVectorValue(t, want.Value, got.Value, "value")
	}
}

func checkVectorPrint(t *testing.T, ast *AST, rule Rule, want vectorPrint) {
	t.Helper()
	canonical := rule.String()
	require.Equal(t, ast.String(), canonical)
	if want.String != nil {
		require.Equal(t, *want.String, canonical, "string")
	}
	check := func(label string, mode PrintMode, want *string, reparse bool) {
		if want == nil {
			return
		}
		got := Format(ast, mode)
		require.Equal(t, *want, got, label)
		require.Equal(t, got, rule.Print(mode), "%s via compiled rule", label)
		if reparse {
			again, err := Parse(got)
			require.NoError(t, err, "%s output must parse", label)
			require.Equal(t, canonical, again.String(), "%s output must re-parse to the same expression", label)
		}
	}
	check("source", Source(), want.Source, false)
	check("compact", Compact(), want.Compact, true)
	check("multiline_2sp", Multiline("  "), want.Multiline2sp, true)
	check("multiline_4sp", Multiline("    "), want.Multiline4sp, true)
	if want.String != nil {
		again, err := Parse(canonical)
		require.NoError(t, err, "string output must parse")
		require.Equal(t, canonical, again.String(), "string output must re-parse to itself")
	}
}

func checkVectorTrace(t *testing.T, path string, got *Trace, want vectorTrace) {
	t.Helper()
	require.NotNil(t, got, path)
	if want.Kind != nil {
		kind := ""
		if got.Node != nil {
			kind = got.Node.Kind().String()
		}
		require.Equal(t, *want.Kind, kind, "%s.kind", path)
	}
	if want.Expr != nil {
		require.Equal(t, *want.Expr, got.Expr, "%s.expr", path)
	}
	if want.Status != nil {
		require.Equal(t, *want.Status, string(got.Status), "%s.status", path)
	}
	if want.Value != nil {
		requireVectorValue(t, want.Value, got.Value, path+".value")
	}
	if want.Active != nil {
		require.Equal(t, *want.Active, got.Active, "%s.active", path)
	}
	if want.Pruned != nil {
		require.Equal(t, *want.Pruned, got.Pruned, "%s.pruned", path)
	}
	if want.MissingFields != nil {
		require.Equal(t, sortedStrings(*want.MissingFields), sortedStrings(got.MissingFields), "%s.missing_fields", path)
	}
	if want.Diagnostics != nil {
		require.Len(t, got.Diagnostics, len(*want.Diagnostics), "%s.diagnostics", path)
		for i, d := range *want.Diagnostics {
			g := got.Diagnostics[i]
			require.Equal(t, d, vectorDiagnostic{Code: string(g.Code), LeftType: g.LeftType, Operator: g.Operator, RightType: g.RightType},
				"%s.diagnostics[%d]", path, i)
			require.NotEmpty(t, g.Message, "%s.diagnostics[%d].message", path, i)
		}
	}
	if want.Children != nil {
		require.Len(t, got.Children, len(*want.Children), "%s.children", path)
		for i, child := range *want.Children {
			checkVectorTrace(t, fmt.Sprintf("%s.children[%d]", path, i), got.Children[i], child)
		}
	}
}

func sortedStrings(s []string) []string {
	out := append([]string{}, s...)
	slices.Sort(out)
	return out
}

// requireVectorValue compares a result value against an expected value written
// in the vector value notation: JSON scalars, arrays, and objects as shorthand,
// and {"$type": ...} objects as typed JSON nodes. Both sides are reduced to the
// canonical typed form before comparing, so type distinctions are kept.
func requireVectorValue(t *testing.T, want json.RawMessage, got any, label string) {
	t.Helper()
	wantValue, err := decodeVectorValue(want)
	require.NoError(t, err, "%s: decoding expected value", label)
	wantJSON, err := canonicalVectorJSON(wantValue)
	require.NoError(t, err, "%s: canonicalizing expected value", label)
	gotJSON, err := canonicalVectorJSON(got)
	require.NoError(t, err, "%s: canonicalizing result value %#v", label, got)
	require.Equal(t, wantJSON, gotJSON, label)
}

func decodeVectorValue(raw json.RawMessage) (any, error) {
	dec := json.NewDecoder(bytes.NewReader(raw))
	dec.UseNumber()
	var v any
	if err := dec.Decode(&v); err != nil {
		return nil, err
	}
	return shorthandVectorValue(v)
}

func shorthandVectorValue(v any) (any, error) {
	switch v := v.(type) {
	case map[string]any:
		if _, typed := v["$type"]; typed {
			return decodeTypedJSONNode(v)
		}
		out := make(map[string]any, len(v))
		for key, child := range v {
			decoded, err := shorthandVectorValue(child)
			if err != nil {
				return nil, err
			}
			out[key] = decoded
		}
		return out, nil
	case []any:
		out := make([]any, len(v))
		for i, child := range v {
			decoded, err := shorthandVectorValue(child)
			if err != nil {
				return nil, err
			}
			out[i] = decoded
		}
		return out, nil
	case json.Number:
		return normalizeJSONNumber(v)
	default:
		return v, nil
	}
}

func canonicalVectorJSON(v any) (string, error) {
	canonical, err := canonicalVectorValue(v)
	if err != nil {
		return "", err
	}
	out, err := json.Marshal(canonical)
	return string(out), err
}

func typedVectorNode(typ, value string) map[string]any {
	return map[string]any{"$type": typ, "value": value}
}

func canonicalVectorValue(v any) (any, error) {
	switch v := v.(type) {
	case nil, bool, string:
		return v, nil
	case int:
		return typedVectorNode("int64", strconv.FormatInt(int64(v), 10)), nil
	case int64:
		return typedVectorNode("int64", strconv.FormatInt(v, 10)), nil
	case uint:
		return typedVectorNode("uint64", strconv.FormatUint(uint64(v), 10)), nil
	case uint64:
		return typedVectorNode("uint64", strconv.FormatUint(v, 10)), nil
	case float64:
		return typedVectorNode("float64", strconv.FormatFloat(v, 'g', -1, 64)), nil
	case net.IP:
		return typedVectorNode("ip", v.String()), nil
	case *net.IPNet:
		return typedVectorNode("cidr", v.String()), nil
	case net.HardwareAddr:
		return typedVectorNode("mac", v.String()), nil
	case *url.URL:
		return typedVectorNode("url", v.String()), nil
	case URL:
		return typedVectorNode("url", v.String()), nil
	case []byte:
		return map[string]any{"$type": "bytes", "encoding": "hex", "value": hex.EncodeToString(v)}, nil
	case HexString:
		return map[string]any{"$type": "bytes", "encoding": "hex", "value": hex.EncodeToString(v.Bytes)}, nil
	case map[string]any:
		out := make(map[string]any, len(v))
		for key, child := range v {
			c, err := canonicalVectorValue(child)
			if err != nil {
				return nil, err
			}
			out[key] = c
		}
		return map[string]any{"$type": "object", "value": out}, nil
	}
	rv := reflect.ValueOf(v)
	if rv.Kind() == reflect.Slice {
		out := make([]any, rv.Len())
		for i := range out {
			c, err := canonicalVectorValue(rv.Index(i).Interface())
			if err != nil {
				return nil, err
			}
			out[i] = c
		}
		return map[string]any{"$type": "array", "value": out}, nil
	}
	return nil, errors.New("value has no vector representation: " + fmt.Sprintf("%T", v))
}
