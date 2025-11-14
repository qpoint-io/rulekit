# Rulekit Language Support for VS Code

Syntax highlighting and language support for [Rulekit](https://github.com/qpoint/rulekit) rule expressions.

## Features

- **Syntax Highlighting** - Full syntax highlighting for `.rk` files
- **Auto-completion** - Code snippets for common patterns
- **Auto-closing** - Auto-close brackets, quotes, and regex delimiters
- **Comment Support** - Line comments (`--`) and block comments (`/* */`)
- **Bracket Matching** - Matching for parentheses and brackets

## Supported Syntax

### Operators

- **Logical**: `and`, `or`, `not`, `&&`, `||`, `!`
- **Comparison**: `==`, `!=`, `<`, `<=`, `>`, `>=`, `eq`, `ne`, `lt`, `le`, `gt`, `ge`
- **Special**: `in`, `contains`, `matches`, `=~`

### Literals

- **Strings**: `"double quoted"` or `'single quoted'`
- **Numbers**: `42`, `3.14`, `-10`, `+5.5`
- **Booleans**: `true`, `false` (case insensitive)
- **IP Addresses**: `192.168.1.1`, `2001:db8::1`
- **CIDR Ranges**: `10.0.0.0/8`, `2001:db8::/32`
- **Hex Strings**: `47:45:54` (e.g., for MAC addresses or hex data)
- **Regex**: `/pattern/` or `|pattern|`

### Examples

```rulekit
-- Simple field comparison
http.method == "GET"

-- Complex rule with multiple conditions
http.status >= 200 and http.status < 300
  and http.method in ["GET", "POST"]

-- Regex matching
domain matches /^example\.com$/

-- CIDR matching
client.ip == 192.168.0.0/16

-- Hex string matching
header.bytes == 47:45:54  -- "GET" in hex

/* Multi-line
   block comment */
process.uid != 0 or tags contains 'internal-svc'
```

## Code Snippets

Type these prefixes and press Tab to expand:

- `cidr` - IP CIDR match
- `match` - Regex match with `/`
- `matchp` - Regex match with `|`
- `contains` - Contains check
- `in` - In set check
- `and` - AND expression
- `or` - OR expression
- `not` - NOT expression
- `eq` - Equality check
- `ne` - Not equal check
- `lt` - Less than
- `gt` - Greater than

## File Extension

This extension activates for files with the `.rk` extension.

## Release Notes

### 0.1.0

Initial release:
- Syntax highlighting
- Code snippets
- Auto-closing pairs
- Comment toggling

## Contributing

Issues and PRs welcome at [github.com/qpoint/rulekit](https://github.com/qpoint/rulekit)

## License

See the main Rulekit repository for license information.

