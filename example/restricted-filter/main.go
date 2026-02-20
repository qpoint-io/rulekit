package main

import (
	"fmt"
	"regexp"
	"strings"

	"github.com/qpoint-io/rulekit"
)

// sqlColumnNameRe matches valid SQL column names: starts with letter or underscore,
// followed by letters, digits, or underscores. Dots are allowed to support
// qualified names (e.g. table.column).
var sqlColumnNameRe = regexp.MustCompile(`^[a-zA-Z_][a-zA-Z0-9_.]*$`)

// sqlSafeOperators are operators that have direct SQL equivalents.
var sqlSafeOperators = map[string]bool{
	"eq":       true, // =
	"ne":       true, // !=
	"gt":       true, // >
	"ge":       true, // >=
	"lt":       true, // <
	"le":       true, // <=
	"and":      true, // AND
	"or":       true, // OR
	"not":      true, // NOT
	"in":       true, // IN
	"contains": true, // LIKE '%...%'
}

// sqlSafeLiteralTypes are literal types that map to SQL value types.
var sqlSafeLiteralTypes = map[string]bool{
	"string":  true,
	"int64":   true,
	"uint64":  true,
	"float64": true,
	"bool":    true,
}

// RestrictToSQL validates that a parsed rule's AST only uses constructs that
// can be converted to a SQL WHERE clause. It returns nil if the rule is safe,
// or an error describing all violations found.
//
// Restrictions enforced:
//   - No function or macro calls
//   - Only SQL-compatible value types (string, int, float, bool)
//   - Field names must be valid SQL column names (letters, digits, underscores, dots)
//   - No regex match operator (=~ / matches)
//   - No standalone value expressions (bare field or literal without a comparison)
//   - Arrays only allowed within IN expressions
func RestrictToSQL(r rulekit.Rule) error {
	node := r.ASTNode()
	var errs []string
	restrictExpr(node, &errs)
	if len(errs) == 0 {
		return nil
	}
	return fmt.Errorf("rule is not SQL-compatible:\n  - %s", strings.Join(errs, "\n  - "))
}

// restrictExpr validates a node in "expression" position — i.e. where a full
// boolean expression is expected (top-level, or as a child of and/or/not).
// Bare fields and literals are rejected here; they're only valid as operands
// of comparison operators.
func restrictExpr(node rulekit.ASTNode, errs *[]string) {
	switch n := node.(type) {
	case *rulekit.ASTNodeOperator:
		restrictOperator(n, errs)
	case *rulekit.ASTNodeField:
		*errs = append(*errs, fmt.Sprintf("standalone field %q is not a valid SQL expression (needs a comparison operator)", n.Name))
	case *rulekit.ASTNodeLiteral:
		*errs = append(*errs, fmt.Sprintf("standalone literal %v is not a valid SQL expression (needs a comparison operator)", n.Value))
	case *rulekit.ASTNodeFunction:
		*errs = append(*errs, fmt.Sprintf("function call %q is not allowed", n.Name))
	default:
		*errs = append(*errs, fmt.Sprintf("unknown AST node type %T", node))
	}
}

// restrictValue validates a node in "value" position — i.e. as an operand
// of a comparison or element of an array.
func restrictValue(node rulekit.ASTNode, errs *[]string) {
	switch n := node.(type) {
	case *rulekit.ASTNodeField:
		if !sqlColumnNameRe.MatchString(n.Name) {
			*errs = append(*errs, fmt.Sprintf("field %q is not a valid SQL column name", n.Name))
		}

	case *rulekit.ASTNodeLiteral:
		if !sqlSafeLiteralTypes[n.Type] {
			*errs = append(*errs, fmt.Sprintf("literal type %q is not SQL-compatible", n.Type))
		}

	case *rulekit.ASTNodeArray:
		for _, elem := range n.Elements {
			restrictValue(elem, errs)
		}

	case *rulekit.ASTNodeFunction:
		*errs = append(*errs, fmt.Sprintf("function call %q is not allowed", n.Name))

	case *rulekit.ASTNodeOperator:
		*errs = append(*errs, fmt.Sprintf("unexpected operator %q in value position", n.Operator))

	default:
		*errs = append(*errs, fmt.Sprintf("unknown AST node type %T", node))
	}
}

func restrictOperator(n *rulekit.ASTNodeOperator, errs *[]string) {
	if !sqlSafeOperators[n.Operator] {
		*errs = append(*errs, fmt.Sprintf("operator %q is not SQL-compatible", n.Operator))
	}

	switch n.Operator {
	case "and", "or":
		// Children of logical operators must be full expressions, not bare values
		restrictExpr(n.Left, errs)
		restrictExpr(n.Right, errs)

	case "not":
		restrictExpr(n.Right, errs)

	case "in":
		restrictValue(n.Left, errs)
		restrictValue(n.Right, errs) // array is allowed as the right operand of IN

	case "eq", "ne", "gt", "ge", "lt", "le", "contains":
		restrictValue(n.Left, errs)
		restrictValue(n.Right, errs)

	case "matches":
		*errs = append(*errs, "regex match (=~) is not SQL-compatible")

	default:
		if n.Left != nil {
			restrictValue(n.Left, errs)
		}
		if n.Right != nil {
			restrictValue(n.Right, errs)
		}
	}
}

func main() {
	tests := []struct {
		name    string
		rule    string
		wantErr bool
	}{
		// Valid SQL-compatible rules
		{name: "simple equality", rule: `port == 8080`},
		{name: "string comparison", rule: `method == "GET"`},
		{name: "and/or logic", rule: `port == 8080 and method == "GET"`},
		{name: "nested parens", rule: `(port == 8080 or port == 443) and method == "POST"`},
		{name: "deeply nested parens", rule: `((port == 80 or port == 443) and method == "GET") or (status >= 400 and status < 500)`},
		{name: "not with parens", rule: `not (port == 8080)`},
		{name: "not nested in parens", rule: `(not (status == 500)) and method == "GET"`},
		{name: "in expression", rule: `method in ["GET", "POST", "PUT"]`},
		{name: "comparison operators", rule: `port >= 1024 and port <= 65535`},
		{name: "contains operator", rule: `path contains "/api"`},
		{name: "boolean literal", rule: `enabled == true`},
		{name: "float literal", rule: `score > 0.5`},
		{name: "dotted field name", rule: `src.port == 8080`},
		{name: "not equals", rule: `status != 404`},

		// Invalid rules
		{name: "function call", rule: `starts_with(path, "/api")`, wantErr: true},
		{name: "function in comparison", rule: `starts_with(path, "/api") == true`, wantErr: true},
		{name: "function nested in and", rule: `port == 80 and starts_with(path, "/api")`, wantErr: true},
		{name: "function nested in parens", rule: `(starts_with(path, "/api")) and port == 80`, wantErr: true},
		{name: "regex match", rule: `method =~ /^GET$/`, wantErr: true},
		{name: "ip literal", rule: `src_ip == 192.168.1.1`, wantErr: true},
		{name: "cidr literal", rule: `src_ip == 192.168.0.0/24`, wantErr: true},
		{name: "hex string literal", rule: `payload contains 50:4f:53:54`, wantErr: true},
		{name: "standalone field", rule: `enabled`, wantErr: true},
		{name: "standalone literal", rule: `true`, wantErr: true},
		{name: "bare field in and", rule: `enabled and port == 80`, wantErr: true},
		{name: "regex nested in parens", rule: `(method =~ /^GET$/) and port == 80`, wantErr: true},
	}

	const (
		green = "\033[32m"
		red   = "\033[31m"
		dim   = "\033[2m"
		cyan  = "\033[36m"
		reset = "\033[0m"
	)

	for _, tt := range tests {
		r, err := rulekit.Parse(tt.rule)
		if err != nil {
			fmt.Printf("%sFAIL%s  %s\n", red, reset, tt.name)
			fmt.Printf("      %s%s%s\n", cyan, tt.rule, reset)
			fmt.Printf("      %sparse error: %v%s\n\n", red, err, reset)
			continue
		}

		err = RestrictToSQL(r)
		pass := (tt.wantErr && err != nil) || (!tt.wantErr && err == nil)

		if pass {
			fmt.Printf("%sPASS%s  %s\n", green, reset, tt.name)
		} else {
			fmt.Printf("%sFAIL%s  %s\n", red, reset, tt.name)
		}

		fmt.Printf("      %s%s%s\n", cyan, tt.rule, reset)

		if err != nil {
			for _, line := range strings.Split(err.Error(), "\n") {
				fmt.Printf("      %s%s%s\n", dim, line, reset)
			}
		}
		fmt.Println()
	}
}
