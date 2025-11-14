# 🔄 Round-trip Demo: Go ↔ TypeScript

This demonstrates perfect round-trip conversion of rules between Go and TypeScript through JSON AST.

## Full Cycle

```
Go Text → Go AST → JSON → TypeScript AST → TypeScript Text → Go Parse ✓
```

## Quick Demo

```bash
# 1. Export rules from Go to JSON
cd go-to-ts-demo
go run main.go

# 2. TypeScript converts JSON back to text
cd ../ts-evaluator
npm run roundtrip

# 3. Go verifies it can parse the regenerated text
cd ../go-to-ts-demo
go run verify-roundtrip.go
```

## Example Output

### Go → JSON

**Input (Go):**
```go
rule := rulekit.MustParse(`port == 8080 and method =~ /^GET|POST$/`)
ast := rule.ASTNode()
```

**Output (JSON):**
```json
{
  "node_type": "operator",
  "operator": "and",
  "left": {
    "node_type": "operator",
    "operator": "eq",
    "left": {"node_type": "field", "name": "port"},
    "right": {"node_type": "literal", "type": "int", "value": 8080}
  },
  "right": {
    "node_type": "operator",
    "operator": "matches",
    "left": {"node_type": "field", "name": "method"},
    "right": {"node_type": "literal", "type": "unknown", "value": "^GET|POST$"}
  }
}
```

### JSON → TypeScript → Text

**TypeScript:**
```typescript
const rule = Rule.fromJSON(json);
const text = rule.toString();
// "port == 8080 and method =~ /^GET|POST$/"
```

### Text → Go (Verify)

**Go:**
```go
regenerated := "port == 8080 and method =~ /^GET|POST$/"
rule, err := rulekit.Parse(regenerated)
// Success! ✅
```

## Why This Matters

1. **Portable Rules** - Define once in Go, use everywhere
2. **Tool Building** - Build rule editors/debuggers in any language
3. **Migration** - Move rules between systems safely
4. **Validation** - Ensure rules work across implementations
5. **Documentation** - Generate human-readable text from AST

## Supported Features

All rulekit features support perfect round-trip:

- ✅ Comparison operators (`==`, `!=`, `>`, `>=`, `<`, `<=`)
- ✅ String operations (`contains`, `matches`)
- ✅ Logical operators (`and`, `or`, `not`)
- ✅ Array operations (`in`, array literals)
- ✅ Nested field access (`request.headers.host`)
- ✅ Functions (`cidr_contains(ip, "10.0.0.0/8")`)
- ✅ All literal types (int, float, string, bool, IP, CIDR, etc.)
- ✅ Special formatting (`!=` instead of `not ==`)
- ✅ Proper parenthesization

## Implementation Details

### TypeScript Stringify

The `stringify()` function in TypeScript:

1. **Mirrors Go formatting** - Uses same string representation
2. **Handles special cases** - `!=`, `not contains`, etc.
3. **Manages parentheses** - Only logical ops get parens
4. **Escapes strings** - Properly handles quotes and backslashes
5. **Formats arrays** - `[1, 2, 3]` format
6. **Regex patterns** - Wraps in `/.../" delimiters

### Key Decisions

**Parentheses:**
- Only `and`/`or` operators wrapped in `(...)`
- Comparison operators have no parens
- Root node outer parens removed

**String Literals:**
- Double-quoted with escaped quotes/backslashes
- `"He said \"hello\""` → `"He said \"hello\""`

**Regex Patterns:**
- Stored as plain string in AST
- Wrapped with `/` on stringify
- `"^GET$"` → `/^GET$/`

**Operators:**
- Use text form: `and`, `or`, `not`, `contains`, `matches`
- Symbols: `==`, `!=`, `>`, `>=`, `<`, `<=`, `=~`, `in`

## Testing

Run the full test suite:

```bash
# Unit tests for stringify
cd ts-evaluator
npm test

# Full round-trip integration test
npm run roundtrip
cd ../go-to-ts-demo
go run verify-roundtrip.go
```

All tests include exact string matching to ensure perfect fidelity.

## Use Cases

### 1. Rule Editor UI

```typescript
// Load rule from backend (Go)
const rule = Rule.fromJSON(astFromAPI);

// Show in UI
document.getElementById("rule-text").value = rule.toString();

// User edits, send back to Go for validation
fetch("/api/validate", { 
  body: JSON.stringify({ expression: rule.toString() })
});
```

### 2. Migration Tool

```bash
# Export all rules from old system
go run export-rules.go > rules.json

# Convert in TypeScript
node convert-format.js < rules.json > new-format.json

# Import to new system (validates by parsing)
go run import-rules.go < new-format.json
```

### 3. Documentation Generator

```typescript
// Generate rule documentation
rules.forEach(rule => {
  console.log(`Rule: ${rule.toString()}`);
  console.log(`Description: ${getDescription(rule)}`);
  console.log(`Fields used: ${extractFields(rule)}`);
});
```

## Limitations

**None!** 🎉

The TypeScript implementation supports 100% of Go rulekit's expression syntax and guarantees perfect round-trip for all valid rules.

## Future Enhancements

While round-trip is perfect, potential improvements:

1. **Formatting options** - Customize spacing, line breaks
2. **Comment preservation** - If we add comment support to AST
3. **Error recovery** - Partial stringify of invalid AST
4. **Performance** - Caching stringified results

But for now, it just works! ✨

