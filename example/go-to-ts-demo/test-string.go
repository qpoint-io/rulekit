package main

import (
	"fmt"

	"github.com/qpoint-io/rulekit"
)

func main() {
	tests := []string{
		`port == 8080 and method =~ /^GET|POST$/`,
		`status in [200, 201, 204]`,
		`host contains "example.com" or host contains "qpoint.io"`,
		`priority > 5 and priority <= 10`,
	}

	for _, expr := range tests {
		rule := rulekit.MustParse(expr)
		output := rule.String()
		fmt.Printf("Input:  %s\n", expr)
		fmt.Printf("Output: %s\n", output)
		fmt.Printf("Match:  %v\n\n", expr == output)
	}
}

