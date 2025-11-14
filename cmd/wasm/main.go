package main

import (
	"encoding/json"
	"syscall/js"

	"github.com/qpoint-io/rulekit"
)

func main() {
	// Expose rulekit API to JavaScript
	js.Global().Set("rulekit", js.ValueOf(map[string]any{
		"parse": js.FuncOf(parseWrapper),
		"eval":  js.FuncOf(evalWrapper),
		"free":  js.FuncOf(freeWrapper),
	}))

	println("rulekit WASM initialized")

	// Keep the Go program running
	<-make(chan struct{})
}

func parseWrapper(this js.Value, args []js.Value) any {
	if len(args) != 1 {
		return map[string]any{
			"error": "expected 1 argument: rule string",
		}
	}

	ruleStr := args[0].String()
	rule, err := rulekit.Parse(ruleStr)

	if err != nil {
		return map[string]any{
			"error": err.Error(),
		}
	}

	// Serialize AST to JSON
	astNode := rule.ASTNode()
	astJSON, err := json.Marshal(astNode)
	if err != nil {
		return map[string]any{
			"error": "failed to serialize AST: " + err.Error(),
		}
	}

	// Store rule and return handle
	handle := storeRule(rule)

	return map[string]any{
		"handle":     handle,
		"ast":        string(astJSON),
		"ruleString": rule.String(),
	}
}

func evalWrapper(this js.Value, args []js.Value) any {
	if len(args) != 2 {
		return map[string]any{
			"error": "expected 2 arguments: rule handle, context object",
		}
	}

	handle := args[0].Int()
	rule := getRule(handle)
	if rule == nil {
		return map[string]any{"error": "invalid rule handle"}
	}

	// Parse context from JS object
	var kv map[string]any
	contextJSON := args[1].String()
	if err := json.Unmarshal([]byte(contextJSON), &kv); err != nil {
		return map[string]any{"error": "invalid context: " + err.Error()}
	}

	ctx := &rulekit.Ctx{
		KV: kv,
	}

	result := rule.Eval(ctx)

	response := map[string]any{
		"ok":   result.Ok(),
		"pass": result.Pass(),
		"fail": result.Fail(),
	}

	// Only include value if it's serializable
	if result.Value != nil {
		if valueJSON, err := json.Marshal(result.Value); err == nil {
			response["value"] = string(valueJSON)
		}
	}

	if result.Error != nil {
		response["error"] = result.Error.Error()
	}

	if result.EvaluatedRule != nil {
		response["evaluatedRule"] = result.EvaluatedRule.String()

		// Include evaluated AST
		if evalASTNode := result.EvaluatedRule.ASTNode(); evalASTNode != nil {
			if evalASTJSON, err := json.Marshal(evalASTNode); err == nil {
				response["evaluatedAST"] = string(evalASTJSON)
			}
		}
	}

	return response
}

func freeWrapper(this js.Value, args []js.Value) any {
	if len(args) != 1 {
		return map[string]any{
			"error": "expected 1 argument: rule handle",
		}
	}

	handle := args[0].Int()
	delete(ruleStore, handle)

	return map[string]any{
		"ok": true,
	}
}

// Simple handle management
var ruleStore = make(map[int]rulekit.Rule)
var nextHandle = 1

func storeRule(r rulekit.Rule) int {
	handle := nextHandle
	nextHandle++
	ruleStore[handle] = r
	return handle
}

func getRule(handle int) rulekit.Rule {
	return ruleStore[handle]
}

