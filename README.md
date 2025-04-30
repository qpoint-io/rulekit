<picture>
  <source media="(prefers-color-scheme: dark)" srcset="./readme_assets/rule-kit-icon-dark.svg">
  <source media="(prefers-color-scheme: light)" srcset="./readme_assets/rule-kit-icon-light.svg">
  <img alt="Rulekit icon" src="./readme_assets/rule-kit-icon-light.svg">
</picture>

# Rulekit

Rulekit is a flexible expression-based rules engine for Go, providing a simple and expressive syntax for defining business rules that can be evaluated against key-value data.

![Rulekit Demo](./readme_assets/demo.gif)

## Overview

This package implements an expression-based rules engine that evaluates expressions against a key-value map of values, returning a true/false result with additional context.

Rules follow a simple and intuitive syntax. For example, the following rule:

```perl
domain matches /example\.com$/
```

When evaluated against:
- `map[string]any{"domain": "example.com"}` → returns **true**
- `map[string]any{"domain": "qpoint.io"}` → returns **false**

In this document, `domain` is referred to as a **field** and `/example\.com$/` as a **value**.

Rulekit supports a flexible syntax where fields and values may appear on either side of an operator:

- `field operator value` (e.g., `domain == "example.com"`)
- `value operator field` (e.g., `"example.com" == domain`)
- `value operator value` (e.g., `123 == 123`)
- `field operator field` (e.g., `src.port == dst.port`)

A field on its own (without an operator) will check if the field contains a non-zero value. For example: `hash && version > 1` will check if the hash field is non-zero and the version is greater than 1.

## Usage Example

```go
import (
    "fmt"
    "github.com/qpoint-io/rulekit/rule"
)

func main() {
    // Parse a rule
    r, err := rule.Parse(`domain matches /example\.com$/ and port == 8080`)
    if err != nil {
        fmt.Printf("Failed to parse rule: %v\n", err)
        return
    }
    
    // Define input data
    inputData := map[string]any{
        "domain": "example.com",
        "port": 8080,
    }
    
    // Evaluate the rule
    result := r.Eval(inputData)
    
    if result.PassStrict() {
        fmt.Println("Rule matched!")
    } else {
        fmt.Println("Rule did not match")
    }
    
    // Check for missing fields (if the result is inconclusive)
    if len(result.MissingFields) > 0 {
        fmt.Printf("Missing fields: %v\n", result.MissingFields)
    }
}
```

## Result

When a rule is evaluated, it returns a `Result` struct containing:

- `Pass`: A boolean indicating if the rule passed
- `MissingFields`: Any fields required by the rule but not present in the input
- `EvaluatedRule`: The rule that was evaluated

The Result also provides additional helper methods:
- `PassStrict()`: Returns true if the rule passes and all required fields are present
- `FailStrict()`: Returns true if the rule fails and all required fields are present
- `Strict()`: Returns true if all required fields are present

## Supported Operators

| Operator | Alias | Description |
|----------|--------------|-------------|
| `or` | `\|\|` | Logical OR |
| `and` | `&&` | Logical AND |
| `not` | `!` | Logical NOT |
| `()` | | Parentheses for grouping |
| `==` | `eq` | Equal to |
| `!=` | `ne` | Not equal to |
| `>` | `gt` | Greater than |
| `>=` | `ge` | Greater than or equal to |
| `<` | `lt` | Less than |
| `<=` | `le` | Less than or equal to |
| `contains` | | Check if a value contains another value |
| `in` | | Check if a value is contained within another value |
| `matches` | | Match against a regular expression |
| `starts_with` | | Check if a string starts with another string |
| `ends_with` | | Check if a string ends with another string |
| `subdomain_of` | | Check if a domain is identical to or is a subdomain of another domain (using public suffix rules) |

## Supported Types

| Type | Used As | Example | Description |
|------|---------|---------|-------------|
| **bool** | VALUE, FIELD | `true` | Valid values: `true`, `false` |
| **number** | VALUE, FIELD | `8080` | Integer or float. Parsed as either int64 or uint64 if out of range for int64, or float64 if float. |
| **string** | VALUE, FIELD | `"domain.com"` | A double-quoted string. Quotes may be escaped with a backslash: `"a string \"with\" quotes"`. Any quoted value is parsed as a string. |
| **IP address** | VALUE, FIELD | `192.168.1.1`, `2001:db8:3333:4444:cccc:dddd:eeee:ffff` | An IPv4, IPv6, or an IPv6 dual address. Maps to Go type: `net.IP` |
| **CIDR** | VALUE | `192.168.1.0/24`, `2001:db8:3333:4444:cccc:dddd:eeee:ffff/64` | An IPv4 or IPv6 CIDR block. Maps to Go type: `*net.IPNet` |
| **Hexadecimal string** | VALUE, FIELD | `12:34:56:78:ab` (MAC address), `504f5354` (hex string "POST") | A hexadecimal string, optionally separated by colons. |
| **Regex** | VALUE | `/example\.com$/` | A Go-style regular expression. Must be surrounded by forward slashes. May not be quoted with double quotes (otherwise it will be parsed as a string). Maps to Go type: `*regexp.Regexp` |

Arrays are also supported using a square bracket notiation. An array may contain mixed value types. For example: `field in [1.2.3.4, "domain.com"]`.

## License

[Apache 2.0](./LICENSE)

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="./readme_assets/qpoint-open.svg">
  <source media="(prefers-color-scheme: light)" srcset="./readme_assets/qpoint-open-light.svg">
  <img alt="Image showing \"Qpoint ❤ OpenSource\"" src="./readme_assets/qpoint-open-light.svg">
</picture>