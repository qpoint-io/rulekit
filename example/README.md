# Rulekit Examples

This directory contains examples demonstrating the rulekit rules engine and cross-language evaluation.

## 📁 Structure

```
example/
├── ast_json/          # Go example showing AST JSON output
├── go-to-ts-demo/     # Go exporter that generates JSON for TypeScript
├── ts-evaluator/      # TypeScript package that evaluates rules
└── vue-demo/          # Interactive Vue.js demo (browser-ready!)
```

## 🚀 Quick Start

### 1. Export Rules from Go

```bash
cd go-to-ts-demo
go run main.go
```

This generates `rules.json` containing AST representations of rules.

### 2. Evaluate in TypeScript

```bash
cd ../ts-evaluator
npm install
npm run demo
```

This loads the JSON and evaluates rules against test data.

## 🔄 Full Workflow

### Go Side (Rule Definition)

```go
// Define rule
rule := rulekit.MustParse(`port == 8080 and method =~ /^GET|POST$/`)

// Export AST as JSON
ast := rule.ASTNode()
jsonBytes, _ := json.Marshal(ast)

// Output:
// {
//   "node_type": "operator",
//   "operator": "and",
//   "left": {
//     "node_type": "operator",
//     "operator": "eq",
//     "left": {"node_type": "field", "name": "port"},
//     "right": {"node_type": "literal", "type": "int", "value": 8080}
//   },
//   ...
// }
```

### TypeScript Side (Rule Evaluation)

```typescript
import { Rule } from "@qpoint/rule-evaluator";

// Load AST from JSON
const rule = Rule.fromJSON(jsonString);

// Evaluate against data
const passes = rule.passes({
  port: 8080,
  method: "GET"
}); // true
```

## 📦 TypeScript Package

The `ts-evaluator` package provides a clean, production-ready implementation:

**Features:**
- ✅ Type-safe AST definitions
- ✅ All operators (==, !=, >, >=, <, <=, contains, matches, in)
- ✅ Logical operators (and, or, not) with short-circuit evaluation
- ✅ Nested field access (dot notation)
- ✅ Custom functions
- ✅ Error handling
- ✅ Zero dependencies for core functionality

**Key Differences from Go:**

| Feature | Go | TypeScript |
|---------|-----|------------|
| **Performance** | Optimized with manual type checks | Clean, idiomatic code |
| **Type Safety** | Compile-time with generics | Runtime with TypeScript types |
| **Operators** | Manual comparison functions | Switch-based dispatch |
| **Field Access** | Iterative path traversal | Simple split + reduce |
| **Code Style** | Verbose, explicit | Concise, functional |

## 🧪 Examples

### Simple Standalone Examples

**ast_json/main.go** - Shows AST JSON output for various rules:

```bash
cd ast_json
go run main.go
```

### Interactive Demo

**ts-evaluator/src/example.ts** - Standalone TypeScript examples:

```bash
cd ts-evaluator
npm run example
```

Shows:
- Simple comparisons
- Array operations
- Nested fields
- Custom functions
- Error handling

### Integration Demo

**go-to-ts-demo/** - Full workflow from Go to TypeScript:

```bash
# Export from Go
cd go-to-ts-demo
go run main.go

# Evaluate in TypeScript
cd ../ts-evaluator
npm run demo
```

## 🎯 Use Cases

1. **Multi-language Services** - Define rules in Go, evaluate in browser/Node.js
2. **Rule Distribution** - Export rules as JSON for multiple consumers
3. **Client-side Filtering** - Send rules to frontend for local evaluation
4. **Edge Computing** - Evaluate rules in CDN edge workers
5. **Testing** - Use TypeScript for easier rule testing/prototyping
6. **Vue/React Apps** - Build interactive rule evaluators in the browser

## 🌐 Browser/Vue.js Support

The TypeScript evaluator works perfectly in browsers and Vue.js apps!

**See:** [VUE_COMPATIBILITY.md](VUE_COMPATIBILITY.md) for details

**Try it:**
```bash
cd vue-demo
npm install
npm run dev
```

Open http://localhost:5173 for an interactive demo! 🎉

## 📚 Documentation

- [Go rulekit](../../README.md) - Main rulekit documentation
- [TypeScript Evaluator](ts-evaluator/README.md) - TypeScript package docs

## 🛠️ Development

### Adding New Node Types

1. **Go**: Add to `ast.go` and implement `ASTNode()` method
2. **TypeScript**: Add to `src/types.ts` and handle in `evaluator.ts`

### Adding New Operators

1. **Go**: Add to `compare.go` and parser
2. **TypeScript**: Add case to `src/operators.ts`

### Adding Custom Functions

**Go:**
```go
ctx := &rulekit.Ctx{
    Functions: map[string]*rulekit.Function{
        "my_func": {
            Args: []rulekit.FunctionArg{{Name: "x"}},
            Eval: func(args map[string]any) rulekit.Result {
                // ...
            },
        },
    },
}
```

**TypeScript:**
```typescript
rule.passes(data, {
    my_func: (x) => {
        // ...
    }
});
```

## 🤝 Contributing

The TypeScript implementation prioritizes clean code over performance. Feel free to:
- Add more operators
- Improve error messages
- Add more examples
- Port to other languages (Python, Rust, etc.)

## 📄 License

MIT

