# Vue.js Rule Evaluator Demo

Interactive Vue 3 app demonstrating the rule evaluator in a browser environment.

## Features

- ✅ **Browser-native** - Runs entirely in the browser, no Node.js required
- ✅ **Composition API** - Modern Vue 3 with `<script setup>`
- ✅ **TypeScript** - Full type safety
- ✅ **Interactive** - Paste AST JSON, test with different data
- ✅ **Examples** - Pre-loaded examples to get started
- ✅ **Live Updates** - See rule text as you edit AST

## Setup

```bash
npm install
npm run dev
```

Then open http://localhost:5173

## Local Development (No npm Publish)

This demo imports directly from the source files using relative paths:

```typescript
// Uses relative import instead of npm package
import { Rule } from '../../ts-evaluator/src/index'
```

This way you can develop and test without publishing to npm!

### Alternative: Using npm link

If you want to simulate a published package locally:

```bash
# In ts-evaluator directory
cd ../ts-evaluator
npm run build
npm link

# In vue-demo directory  
cd ../vue-demo
npm link @qpoint/rule-evaluator

# Then use normal imports
import { Rule } from '@qpoint/rule-evaluator'
```

## Usage in Your Vue App (After Publishing)

### 1. Install the package

```bash
npm install @qpoint/rule-evaluator
```

### 2. Import and use

```vue
<script setup lang="ts">
import { ref } from 'vue'
import { Rule } from '@qpoint/rule-evaluator'

const astJson = ref('...')  // AST from your backend
const data = ref({ port: 8080, method: 'GET' })

function evaluateRule() {
  const rule = Rule.fromJSON(JSON.parse(astJson.value))
  const result = rule.eval(data.value)
  console.log(result.value ? 'PASS' : 'FAIL')
}
</script>
```

## Using with Composition API

### As a Composable

Create `composables/useRuleEvaluator.ts`:

```typescript
import { ref, computed } from 'vue'
import { Rule } from '@qpoint/rule-evaluator'
import type { ASTNode, EvalResult } from '@qpoint/rule-evaluator'

export function useRuleEvaluator(ast: ASTNode) {
  const rule = Rule.fromJSON(ast)
  const data = ref<Record<string, any>>({})
  
  const result = computed<EvalResult>(() => {
    return rule.eval(data.value)
  })
  
  const passes = computed(() => rule.passes(data.value))
  const ruleText = rule.toString()
  
  return {
    data,
    result,
    passes,
    ruleText,
    evaluate: () => rule.eval(data.value)
  }
}
```

Usage:

```vue
<script setup lang="ts">
import { useRuleEvaluator } from './composables/useRuleEvaluator'

const { data, passes, ruleText } = useRuleEvaluator(myAST)

data.value = { port: 8080, method: 'GET' }
</script>

<template>
  <div>
    <p>Rule: {{ ruleText }}</p>
    <p>Result: {{ passes ? '✅ Pass' : '❌ Fail' }}</p>
  </div>
</template>
```

## Fetching Rules from Backend

```vue
<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { Rule } from '@qpoint/rule-evaluator'

const rules = ref<Rule[]>([])

onMounted(async () => {
  // Fetch rules from your Go backend
  const response = await fetch('/api/rules')
  const { rules: astList } = await response.json()
  
  // Convert AST to Rule objects
  rules.value = astList.map((ast: any) => Rule.fromJSON(ast))
})

function checkRule(rule: Rule, data: any) {
  return rule.passes(data)
}
</script>
```

## Reactive Rule Evaluation

```vue
<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { Rule } from '@qpoint/rule-evaluator'

const rule = Rule.fromJSON(myAST)
const formData = ref({
  port: 8080,
  method: 'GET',
  host: 'example.com'
})

// Auto-evaluate when data changes
const isValid = computed(() => rule.passes(formData.value))

// Show validation message
watch(isValid, (valid) => {
  if (valid) {
    console.log('✅ Validation passed!')
  } else {
    console.log('❌ Validation failed')
  }
})
</script>
```

## Bundle Size

The rule evaluator is lightweight and tree-shakeable:

- **Core library:** ~15KB minified
- **No dependencies** (runs in browser)
- **Tree-shakeable** - Import only what you need

## Browser Compatibility

Works in all modern browsers:
- Chrome/Edge 90+
- Firefox 88+
- Safari 14+

## Common Patterns

### Form Validation

```vue
<script setup lang="ts">
import { Rule } from '@qpoint/rule-evaluator'

const validationRule = Rule.fromJSON(astFromBackend)

function validateForm(formData: any) {
  const result = validationRule.eval(formData)
  
  if (!result.ok) {
    return { valid: false, error: result.error }
  }
  
  return { valid: result.value, error: null }
}
</script>
```

### Dynamic Rules

```vue
<script setup lang="ts">
import { ref, watchEffect } from 'vue'
import { Rule } from '@qpoint/rule-evaluator'

const currentRule = ref<Rule | null>(null)
const ruleAST = ref<any>(null)

// Re-create rule when AST changes
watchEffect(() => {
  if (ruleAST.value) {
    currentRule.value = Rule.fromJSON(ruleAST.value)
  }
})
</script>
```

## TypeScript Support

Full TypeScript support out of the box:

```typescript
import type { 
  ASTNode, 
  EvalResult, 
  Context 
} from '@qpoint/rule-evaluator'

const ast: ASTNode = { /* ... */ }
const result: EvalResult = rule.eval(data)
```

## Production Tips

1. **Cache rules** - Parse AST once, reuse Rule objects
2. **Validate data** - Check data structure before evaluation
3. **Error handling** - Always check `result.ok` before using `result.value`
4. **Performance** - Rules are fast, but cache repeated evaluations if needed

## License

MIT

