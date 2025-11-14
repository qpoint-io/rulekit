package main

import (
	"encoding/json"
	"fmt"
	"os"

	"github.com/qpoint-io/rulekit"
)

func main() {
	// Define rules in Go
	rules := []string{
		`port == 8080 and method =~ /^GET|POST$/`,
		`status in [200, 201, 204]`,
		`(tags contains "production" or tags contains "staging") and priority > 5`,
		`request.method == "POST" and request.headers.content-type contains "json"`,
	}

	// Export ASTs
	asts := make([]any, len(rules))
	for i, ruleStr := range rules {
		rule := rulekit.MustParse(ruleStr)
		asts[i] = map[string]any{
			"expression": ruleStr,
			"ast":        rule.ASTNode(),
		}
	}

	// Write to JSON file for TypeScript to consume
	output := map[string]any{
		"rules": asts,
	}

	jsonBytes, err := json.MarshalIndent(output, "", "  ")
	if err != nil {
		panic(err)
	}

	err = os.WriteFile("rules.json", jsonBytes, 0644)
	if err != nil {
		panic(err)
	}

	fmt.Println("✅ Exported", len(rules), "rules to rules.json")
	fmt.Println("\nRules:")
	for i, ruleStr := range rules {
		fmt.Printf("  %d. %s\n", i+1, ruleStr)
	}
	fmt.Println("\n💡 Run the TypeScript evaluator with: cd ../ts-evaluator && npm run demo")
}

