//go:build js && wasm

package main

import (
	"context"
	"encoding/json"
	"fmt"
	"strings"
	"syscall/js"

	rulekit "github.com/qpoint-io/rulekit/v2"
)

type parseResponse struct {
	OK        bool                `json:"ok"`
	Source    string              `json:"source,omitempty"`
	Compact   string              `json:"compact,omitempty"`
	Multiline string              `json:"multiline,omitempty"`
	AST       *rulekit.JSONNode   `json:"ast,omitempty"`
	Tokens    []rulekit.JSONToken `json:"tokens,omitempty"`
	Error     string              `json:"error,omitempty"`
}

type sourceResponse struct {
	OK     bool              `json:"ok"`
	Source string            `json:"source,omitempty"`
	AST    *rulekit.JSONNode `json:"ast,omitempty"`
	Error  string            `json:"error,omitempty"`
}

type nodeRef struct {
	ID    string `json:"id"`
	Start int    `json:"start"`
	End   int    `json:"end"`
}

type editRequest struct {
	Target      nodeRef `json:"target"`
	Replacement string  `json:"replacement"`
	Kind        string  `json:"kind"`
	Mode        string  `json:"mode"`
}

type evalResponse struct {
	OK            bool              `json:"ok"`
	Value         any               `json:"value,omitempty"`
	Status        string            `json:"status,omitempty"`
	Error         string            `json:"error,omitempty"`
	MissingFields []string          `json:"missingFields,omitempty"`
	Trace         *traceDTO         `json:"trace,omitempty"`
	AST           *rulekit.JSONNode `json:"ast,omitempty"`
}

type traceDTO struct {
	Kind          string               `json:"kind,omitempty"`
	Expr          string               `json:"expr,omitempty"`
	Value         any                  `json:"value,omitempty"`
	Error         string               `json:"error,omitempty"`
	MissingFields []string             `json:"missingFields,omitempty"`
	Diagnostics   []rulekit.Diagnostic `json:"diagnostics,omitempty"`
	Status        string               `json:"status,omitempty"`
	Active        bool                 `json:"active,omitempty"`
	Pruned        bool                 `json:"pruned,omitempty"`
	Span          *rulekit.JSONSpan    `json:"span,omitempty"`
	Children      []*traceDTO          `json:"children,omitempty"`
}

type foundNode struct {
	node        rulekit.ASTNode
	parent      rulekit.ASTNode
	childIndex  int
	id          string
	parentID    string
	childrenLen int
}

func main() {
	api := js.Global().Get("Object").New()
	api.Set("parse", js.FuncOf(func(this js.Value, args []js.Value) any {
		return toJSON(parseRule(argString(args, 0)))
	}))
	api.Set("format", js.FuncOf(func(this js.Value, args []js.Value) any {
		return toJSON(formatRule(argString(args, 0), argString(args, 1)))
	}))
	api.Set("rewrite", js.FuncOf(func(this js.Value, args []js.Value) any {
		return toJSON(rewriteRule(argString(args, 0), argString(args, 1)))
	}))
	api.Set("deleteNode", js.FuncOf(func(this js.Value, args []js.Value) any {
		return toJSON(deleteNode(argString(args, 0), argString(args, 1)))
	}))
	api.Set("evalRule", js.FuncOf(func(this js.Value, args []js.Value) any {
		return toJSON(evalRule(argString(args, 0), argString(args, 1)))
	}))
	js.Global().Set("rulekitWasm", api)
	select {}
}

func argString(args []js.Value, index int) string {
	if index >= len(args) || args[index].IsUndefined() || args[index].IsNull() {
		return ""
	}
	return args[index].String()
}

func toJSON(value any) string {
	data, err := json.Marshal(value)
	if err != nil {
		data, _ = json.Marshal(map[string]any{"ok": false, "error": err.Error()})
	}
	return string(data)
}

func parseRule(source string) parseResponse {
	ast, err := rulekit.ParseAST(source)
	if err != nil {
		return parseResponse{OK: false, Source: source, Error: err.Error()}
	}
	doc := ast.JSON()
	return parseResponse{
		OK:        true,
		Source:    source,
		Compact:   rulekit.Format(ast, rulekit.Compact()),
		Multiline: rulekit.Format(ast, rulekit.Multiline("  ")),
		AST:       doc.Root,
		Tokens:    doc.Tokens,
	}
}

func formatRule(source string, mode string) sourceResponse {
	ast, err := rulekit.ParseAST(source)
	if err != nil {
		return sourceResponse{OK: false, Error: err.Error()}
	}
	out := rulekit.Format(ast, printMode(mode))
	parsed := parseRule(out)
	return sourceResponse{OK: parsed.OK, Source: out, AST: parsed.AST, Error: parsed.Error}
}

func rewriteRule(source string, raw string) parseResponse {
	var req editRequest
	if err := json.Unmarshal([]byte(raw), &req); err != nil {
		return parseResponse{OK: false, Source: source, Error: err.Error()}
	}
	ast, err := rulekit.ParseAST(source)
	if err != nil {
		return parseResponse{OK: false, Source: source, Error: err.Error()}
	}
	found := findNode(ast.Root(), req.Target)
	if found == nil {
		return parseResponse{OK: false, Source: source, Error: "target node not found"}
	}

	var out string
	if req.Kind == "operator" {
		out, err = rewriteOperator(source, found.node, req.Replacement)
	} else {
		var replacement *rulekit.AST
		replacement, err = rulekit.ParseAST(req.Replacement)
		if err == nil {
			out, err = rulekit.Rewrite(ast, []rulekit.Edit{{Target: found.node, Replacement: replacement}}, printMode(req.Mode))
		}
	}
	if err != nil {
		return parseResponse{OK: false, Source: source, Error: err.Error()}
	}
	return parseRule(out)
}

func deleteNode(source string, raw string) parseResponse {
	var target nodeRef
	if err := json.Unmarshal([]byte(raw), &target); err != nil {
		return parseResponse{OK: false, Source: source, Error: err.Error()}
	}
	ast, err := rulekit.ParseAST(source)
	if err != nil {
		return parseResponse{OK: false, Source: source, Error: err.Error()}
	}
	found := findNode(ast.Root(), target)
	if found == nil {
		return parseResponse{OK: false, Source: source, Error: "target node not found"}
	}
	if found.parent == nil {
		return parseResponse{OK: false, Source: source, Error: "cannot delete the root expression"}
	}

	var replacementSource string
	switch found.parent.Kind() {
	case rulekit.ASTBinary, rulekit.ASTUnary:
		children := found.parent.Children()
		if found.parent.Kind() == rulekit.ASTBinary && len(children) == 2 {
			op := rulekit.NodeOperator(found.parent)
			if op != rulekit.OperatorAnd && op != rulekit.OperatorOr {
				return parseResponse{OK: false, Source: source, Error: "cannot delete part of a comparison"}
			}
			replacementSource = sourceForNode(source, children[1-found.childIndex])
		} else if found.parent.Kind() == rulekit.ASTUnary && len(children) == 1 {
			replacementSource = sourceForNode(source, children[0])
		} else {
			return parseResponse{OK: false, Source: source, Error: "cannot delete this node"}
		}
		return rewriteNode(source, ast, found.parent, replacementSource)
	case rulekit.ASTArray:
		children := found.parent.Children()
		if len(children) <= 1 {
			return parseResponse{OK: false, Source: source, Error: "array requires at least one value"}
		}
		parts := make([]string, 0, len(children)-1)
		for i, child := range children {
			if i != found.childIndex {
				parts = append(parts, sourceForNode(source, child))
			}
		}
		return rewriteNode(source, ast, found.parent, "["+strings.Join(parts, ", ")+"]")
	default:
		return parseResponse{OK: false, Source: source, Error: "cannot delete this node"}
	}
}

func rewriteNode(source string, ast *rulekit.AST, target rulekit.ASTNode, replacementSource string) parseResponse {
	replacement, err := rulekit.ParseAST(replacementSource)
	if err != nil {
		return parseResponse{OK: false, Source: source, Error: err.Error()}
	}
	out, err := rulekit.Rewrite(ast, []rulekit.Edit{{Target: target, Replacement: replacement}}, rulekit.Source())
	if err != nil {
		return parseResponse{OK: false, Source: source, Error: err.Error()}
	}
	return parseRule(out)
}

func evalRule(source string, inputJSON string) evalResponse {
	ast, err := rulekit.ParseAST(source)
	if err != nil {
		return evalResponse{OK: false, Error: err.Error()}
	}
	rule, err := rulekit.Compile(ast)
	if err != nil {
		return evalResponse{OK: false, Error: err.Error(), AST: ast.JSON().Root}
	}
	input, err := rulekit.DecodeJSON([]byte(inputJSON), rulekit.JSONOptions{})
	if err != nil {
		return evalResponse{OK: false, Error: "input json: " + err.Error(), AST: ast.JSON().Root}
	}
	res := rule.Eval(context.Background(), rulekit.FromKV(input), rulekit.Opts{Trace: true})
	out := evalResponse{
		OK:            res.Error == nil,
		Value:         res.Value,
		Status:        resultStatus(res),
		MissingFields: res.MissingFields,
		Trace:         buildTraceDTO(ast, res.Trace),
		AST:           ast.JSON().Root,
	}
	if res.Error != nil {
		out.Error = res.Error.Error()
	}
	return out
}

func resultStatus(res rulekit.Result) string {
	switch {
	case res.Error != nil:
		return "error"
	case len(res.MissingFields) > 0:
		return "missing"
	case res.Pass():
		return "passed"
	case res.Fail():
		return "failed"
	default:
		return "unknown"
	}
}

func printMode(mode string) rulekit.PrintMode {
	if mode == "multiline" {
		return rulekit.Multiline("  ")
	}
	if mode == "source" {
		return rulekit.Source()
	}
	return rulekit.Compact()
}

func buildTraceDTO(ast *rulekit.AST, trace *rulekit.Trace) *traceDTO {
	if trace == nil {
		return nil
	}
	out := &traceDTO{
		Expr:          trace.Expr,
		Value:         trace.Value,
		MissingFields: trace.MissingFields,
		Diagnostics:   trace.Diagnostics,
		Status:        string(trace.Status),
		Active:        trace.Active,
		Pruned:        trace.Pruned,
	}
	if trace.Error != nil {
		out.Error = trace.Error.Error()
	}
	if trace.Node != nil {
		out.Kind = trace.Node.Kind().String()
		span := ast.JSONSpan(trace.Node.Span())
		out.Span = &span
	}
	for _, child := range trace.Children {
		out.Children = append(out.Children, buildTraceDTO(ast, child))
	}
	return out
}

func findNode(root rulekit.ASTNode, ref nodeRef) *foundNode {
	var found *foundNode
	var walk func(node rulekit.ASTNode, id string, parent rulekit.ASTNode, parentID string, index int)
	walk = func(node rulekit.ASTNode, id string, parent rulekit.ASTNode, parentID string, index int) {
		if node == nil || found != nil {
			return
		}
		span := node.Span()
		if (ref.ID != "" && id == ref.ID) || (span.Start == ref.Start && span.End == ref.End) {
			parentLen := 0
			if parent != nil {
				parentLen = len(parent.Children())
			}
			found = &foundNode{node: node, parent: parent, childIndex: index, id: id, parentID: parentID, childrenLen: parentLen}
			return
		}
		for i, child := range node.Children() {
			walk(child, fmt.Sprintf("%s.%d", id, i), node, id, i)
		}
	}
	walk(root, "root", nil, "", -1)
	return found
}

func rewriteOperator(source string, node rulekit.ASTNode, next string) (string, error) {
	children := node.Children()
	if len(children) != 2 {
		return "", fmt.Errorf("operator rewrite requires a binary node")
	}
	raw := rulekit.NodeRawOperator(node)
	if raw == "" {
		raw = rulekit.NodeOperator(node).String()
	}
	left := children[0].Span()
	right := children[1].Span()
	betweenStart, betweenEnd := left.End, right.Start
	if betweenStart > betweenEnd || betweenEnd > len(source) {
		return "", fmt.Errorf("operator span is outside source")
	}
	between := source[betweenStart:betweenEnd]
	rel := strings.Index(between, raw)
	if rel < 0 {
		return "", fmt.Errorf("operator token not found")
	}
	start := betweenStart + rel
	end := start + len(raw)
	return source[:start] + next + source[end:], nil
}

func sourceForNode(source string, node rulekit.ASTNode) string {
	span := node.Span()
	if span.Start < 0 || span.End > len(source) || span.End < span.Start {
		return node.String()
	}
	return source[span.Start:span.End]
}
