# Using rulekit WASM in the Vue Demo

This demo now supports evaluating rules using WebAssembly (WASM) in addition to the TypeScript evaluator.

## Quick Start

1. **Build the WASM module** (from rulekit root):
   ```bash
   make wasm
   ```

2. **Start the dev server**:
   ```bash
   npm run dev
   ```

3. **Use in your Vue components**:
   ```typescript
   import { rulekit } from './rulekit-wasm'
   
   // Parse and evaluate
   const parsed = await rulekit.parse('domain == "example.com"')
   const result = await rulekit.eval(parsed.handle!, { domain: 'example.com' })
   console.log(result.pass) // true
   ```

## Example Integration

Here's a complete example showing how to integrate WASM evaluation:

```vue
<script setup lang="ts">
import { ref, computed } from 'vue'
import { rulekit } from './rulekit-wasm'

const ruleText = ref('port == 443 && domain contains "api"')
const contextJSON = ref(JSON.stringify({ port: 443, domain: 'api.example.com' }, null, 2))
const result = ref<any>(null)
const loading = ref(false)
const error = ref<string | null>(null)

async function evaluateWasm() {
  loading.value = true
  error.value = null
  
  try {
    // Parse the rule
    const parsed = await rulekit.parse(ruleText.value)
    
    if (parsed.error) {
      error.value = parsed.error
      return
    }

    console.log('Parsed AST:', parsed.ast)
    
    // Parse context
    const context = JSON.parse(contextJSON.value)
    
    // Evaluate
    result.value = await rulekit.eval(parsed.handle!, context)
    
    // Clean up
    await rulekit.free(parsed.handle!)
    
  } catch (e) {
    error.value = String(e)
  } finally {
    loading.value = false
  }
}

const resultColor = computed(() => {
  if (!result.value) return ''
  return result.value.pass ? 'green' : 'red'
})
</script>

<template>
  <div class="wasm-evaluator">
    <h2>🚀 WASM Evaluator</h2>
    
    <div class="input-group">
      <label>Rule Expression:</label>
      <input v-model="ruleText" placeholder="Enter rule..." />
    </div>
    
    <div class="input-group">
      <label>Context (JSON):</label>
      <textarea v-model="contextJSON" rows="6"></textarea>
    </div>
    
    <button @click="evaluateWasm" :disabled="loading">
      {{ loading ? 'Evaluating...' : 'Evaluate with WASM' }}
    </button>
    
    <div v-if="error" class="error">
      ❌ Error: {{ error }}
    </div>
    
    <div v-if="result" class="result" :style="{ color: resultColor }">
      <h3>Result: {{ result.pass ? '✅ PASS' : '❌ FAIL' }}</h3>
      <pre>{{ result.evaluatedRule }}</pre>
      
      <details>
        <summary>Details</summary>
        <pre>{{ JSON.stringify(result, null, 2) }}</pre>
      </details>
    </div>
  </div>
</template>

<style scoped>
.wasm-evaluator {
  padding: 20px;
  border: 2px solid #42b883;
  border-radius: 8px;
  margin: 20px 0;
}

.input-group {
  margin: 10px 0;
}

.input-group label {
  display: block;
  font-weight: bold;
  margin-bottom: 5px;
}

.input-group input,
.input-group textarea {
  width: 100%;
  padding: 8px;
  font-family: monospace;
  border: 1px solid #ccc;
  border-radius: 4px;
}

button {
  padding: 10px 20px;
  background: #42b883;
  color: white;
  border: none;
  border-radius: 4px;
  cursor: pointer;
  font-size: 16px;
  margin: 10px 0;
}

button:disabled {
  background: #ccc;
  cursor: not-allowed;
}

.error {
  color: red;
  padding: 10px;
  background: #ffeeee;
  border-radius: 4px;
  margin: 10px 0;
}

.result {
  margin: 20px 0;
  padding: 10px;
  background: #f5f5f5;
  border-radius: 4px;
}

.result pre {
  white-space: pre-wrap;
  word-break: break-word;
}
</style>
```

## API Reference

### Parse

```typescript
const result = await rulekit.parse('domain == "example.com"')
// Returns:
// {
//   handle: 1,
//   ast: { /* AST structure */ },
//   ruleString: 'domain == "example.com"'
// }
// OR { error: "parse error message" }
```

### Eval

```typescript
const result = await rulekit.eval(handle, { domain: "example.com" })
// Returns:
// {
//   ok: true,
//   pass: true,
//   fail: false,
//   value: true,
//   evaluatedRule: '"example.com" == "example.com"',
//   evaluatedAST: { /* evaluated AST */ }
// }
```

### Free

```typescript
await rulekit.free(handle)
// Releases memory for the rule
```

## Performance Comparison

| Operation | TypeScript | WASM | Notes |
|-----------|-----------|------|-------|
| Init      | 0ms       | 100-200ms | WASM one-time init cost |
| Parse     | 1-5ms     | <1ms | WASM faster for complex rules |
| Eval      | 1-2ms     | <1ms | Similar performance |
| Bundle    | ~100KB    | ~4.4MB | WASM includes Go runtime |

**When to use WASM:**
- ✅ Exact parity with Go backend behavior
- ✅ Complex rules with many operations
- ✅ Validation before sending to backend
- ✅ Need accurate AST representation

**When to use TypeScript:**
- ✅ Simple rules
- ✅ Bundle size matters
- ✅ Instant initialization
- ✅ Client-side only, no backend sync needed

## Troubleshooting

### WASM file not found
Run `make wasm` from the rulekit root directory.

### Module not initializing
Check browser console for errors. WASM requires modern browsers with WebAssembly support.

### Memory leaks
Always call `rulekit.free(handle)` when done with a rule. Consider using try/finally:

```typescript
let handle: number | undefined
try {
  const parsed = await rulekit.parse(rule)
  handle = parsed.handle
  // ... use the rule
} finally {
  if (handle) await rulekit.free(handle)
}
```

## Examples

See `src/rulekit-example.ts` for runnable examples, or run:

```typescript
import { runExamples } from './rulekit-example'
await runExamples() // Logs examples to console
```

