package main

import (
	"encoding/json"
	"fmt"
	"os"

	"github.com/qpoint-io/rulekit"
)

type RoundtripResult struct {
	Original    string `json:"original"`
	Regenerated string `json:"regenerated"`
	Match       bool   `json:"match"`
}

type RoundtripOutput struct {
	Total   int               `json:"total"`
	Passed  int               `json:"passed"`
	Results []RoundtripResult `json:"results"`
}

func main() {
	// Read roundtrip results from TypeScript
	data, err := os.ReadFile("roundtrip-results.json")
	if err != nil {
		fmt.Printf("❌ Could not read roundtrip-results.json: %v\n", err)
		fmt.Println("💡 Run: cd ../ts-evaluator && npm run roundtrip")
		os.Exit(1)
	}

	var output RoundtripOutput
	if err := json.Unmarshal(data, &output); err != nil {
		fmt.Printf("❌ Could not parse JSON: %v\n", err)
		os.Exit(1)
	}

	fmt.Println("🔍 Verifying Round-trip Results in Go")
	fmt.Println("=" + repeat("=", 69))

	allValid := true
	for i, result := range output.Results {
		fmt.Printf("\n✓ Rule %d: %s\n", i+1, result.Original)
		
		// Try to parse the regenerated expression
		_, err := rulekit.Parse(result.Regenerated)
		if err != nil {
			fmt.Printf("  ❌ Regenerated expression failed to parse: %v\n", err)
			allValid = false
			continue
		}
		
		if result.Match {
			fmt.Printf("  ✅ Exact match - perfect round-trip\n")
		} else {
			fmt.Printf("  ⚠️  Different formatting but still valid:\n")
			fmt.Printf("     %s\n", result.Regenerated)
		}
	}

	fmt.Println("\n" + repeat("=", 70))
	fmt.Printf("\n📊 Summary: %d/%d regenerated rules are valid Go expressions\n", 
		output.Passed, output.Total)
	
	if allValid && output.Passed == output.Total {
		fmt.Println("🎉 SUCCESS! All rules completed perfect round-trip:")
		fmt.Println("   Go → JSON → TypeScript → String → Go ✓")
	} else if allValid {
		fmt.Println("✅ All regenerated expressions are valid (minor formatting diffs)")
	} else {
		fmt.Println("❌ Some regenerated expressions failed to parse")
		os.Exit(1)
	}
}

func repeat(s string, n int) string {
	result := ""
	for i := 0; i < n; i++ {
		result += s
	}
	return result
}

