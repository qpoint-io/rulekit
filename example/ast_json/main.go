package main

import (
	"encoding/json"
	"fmt"

	"github.com/qpoint-io/rulekit"
)

func main() {
	examples := []string{
		`port == 8080 and method =~ /^GET|POST$/`,
		`status in [200, 201, 204]`,
		`(host contains "example.com" or host contains "qpoint.io") and !blocked`,
		`src.ip == 192.168.1.1 and dst.port > 1024`,
	}

	for i, expr := range examples {
		fmt.Printf("Example %d: %s\n", i+1, expr)
		fmt.Println("=" + string(make([]byte, 60))[:60])
		
		rule := rulekit.MustParse(expr)
		ast := rule.ASTNode()
		
		jsonBytes, _ := json.MarshalIndent(ast, "", "  ")
		fmt.Println(string(jsonBytes))
		fmt.Println()
	}
}

