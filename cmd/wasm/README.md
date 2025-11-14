# rulekit WASM

This package compiles the rulekit Go package to WebAssembly (WASM) for use in web browsers.

## Building

From the rulekit root directory:

```bash
make wasm
```

Or directly:

```bash
cd cmd/wasm
./build.sh
```

This will:
1. Compile the Go code to WASM (`rulekit.wasm`)
2. Copy the Go WASM runtime (`wasm_exec.js`)
3. Place both files in `example/vue-demo/public/`

## Usage

### TypeScript/JavaScript

```typescript
import { rulekit } from './rulekit-wasm'

// Parse a rule
const parsed = await rulekit.parse('domain == "example.com"')

if (parsed.error) {
  console.error('Parse error:', parsed.error)
} else {
  console.log('Rule:', parsed.ruleString)
  console.log('AST:', parsed.ast)

  // Evaluate the rule
  const result = await rulekit.eval(parsed.handle, {
    domain: 'example.com'
  })

  console.log('Pass:', result.pass)
  console.log('Evaluated:', result.evaluatedRule)

  // Clean up when done
  await rulekit.free(parsed.handle)
}
```

### Vue.js

```vue
<script setup lang="ts">
import { ref } from 'vue'
import { rulekit } from './rulekit-wasm'

const ruleText = ref('port == 443 && domain contains "api"')
const context = ref({ port: 443, domain: 'api.example.com' })
const result = ref<any>(null)

async function evaluate() {
  const parsed = await rulekit.parse(ruleText.value)
  
  if (parsed.error) {
    result.value = { error: parsed.error }
    return
  }

  result.value = await rulekit.eval(parsed.handle!, context.value)
  await rulekit.free(parsed.handle!)
}
</script>

<template>
  <div>
    <input v-model="ruleText" placeholder="Enter rule" />
    <button @click="evaluate">Evaluate</button>
    <div v-if="result">
      <p>Result: {{ result.pass ? '✅ PASS' : '❌ FAIL' }}</p>
      <pre>{{ result.evaluatedRule }}</pre>
    </div>
  </div>
</template>
```

## API

### `rulekit.parse(rule: string)`

Parses a rule expression string.

**Returns:** `ParseResult`
- `handle: number` - Handle to the parsed rule (use with `eval()`)
- `ast: any` - Abstract syntax tree of the parsed rule
- `ruleString: string` - Normalized string representation of the rule
- `error?: string` - Parse error message if parsing failed

### `rulekit.eval(handle: number, context: Record<string, any>)`

Evaluates a parsed rule with the given context.

**Returns:** `EvalResult`
- `ok: boolean` - True if evaluation completed without error
- `pass: boolean` - True if the rule evaluated to a truthy value
- `fail: boolean` - True if the rule evaluated to a falsy value
- `value?: any` - The result value (if serializable)
- `evaluatedRule?: string` - String representation of the evaluated rule
- `evaluatedAST?: any` - AST of the evaluated rule
- `error?: string` - Error message if evaluation failed

### `rulekit.free(handle: number)`

Releases the memory associated with a rule handle. Good practice to call when done with a rule.

### `rulekit.isReady()`

Returns `true` if the WASM module is initialized.

### `rulekit.waitReady()`

Returns a promise that resolves when the WASM module is ready.

## Architecture

```
┌─────────────────┐
│   Vue.js App    │
│  (TypeScript)   │
└────────┬────────┘
         │
         │ import rulekit
         ▼
┌─────────────────┐
│ rulekit-wasm.ts │  TypeScript wrapper
│   (thin layer)  │  - Type definitions
│                 │  - JSON serialization
└────────┬────────┘  - Promise-based API
         │
         │ JS interop
         ▼
┌─────────────────┐
│  rulekit.wasm   │  Compiled Go code
│   (Go/WASM)     │  - Parser
│                 │  - Evaluator
│                 │  - AST generation
└─────────────────┘
```

## Performance

- **Bundle size:** ~2-3 MB (includes Go runtime)
- **Init time:** ~100-200ms on first load (cached by browser)
- **Parse time:** <1ms for typical rules
- **Eval time:** <1ms for typical evaluations

## Debugging

The WASM module logs to the browser console. Check for messages like:
- `✅ rulekit WASM loaded successfully` - Module initialized
- `❌ Failed to initialize rulekit WASM` - Initialization error

Use browser DevTools to debug:
```javascript
// Check if WASM is loaded
console.log(window.rulekit)

// Test directly from console
window.rulekit.parse('domain == "test"')
```

## Limitations

1. **No custom functions** - The WASM module only supports stdlib functions
2. **No macros** - Custom macros must be expanded before passing to WASM
3. **Serialization** - Complex types (IP, Regex) are serialized as strings/objects
4. **Memory** - Rule handles must be freed manually (no automatic GC from JS side)

## Development

To modify the WASM module:

1. Edit `main.go`
2. Run `./build.sh`
3. Refresh your browser (WASM is cached aggressively)

To see WASM output:
```go
// In main.go, use println() to log to browser console
println("Debug message from Go!")
```

