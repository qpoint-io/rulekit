<script setup lang="ts">
import { ref, computed, watch, shallowRef, onMounted } from 'vue'
import { Codemirror } from 'vue-codemirror'
import { json } from '@codemirror/lang-json'
import { oneDark } from '@codemirror/theme-one-dark'
import { Rule } from '../../ts-evaluator/src/index'
import type { EvalResult } from '../../ts-evaluator/src/types'

// Sample AST from Go
// TestEngineExample from rule_test.go
const sampleAST = {"node_type":"operator","operator":"or","left":{"node_type":"operator","operator":"or","left":{"node_type":"operator","operator":"or","left":{"node_type":"operator","operator":"or","left":{"node_type":"operator","operator":"eq","left":{"node_type":"field","name":"tags"},"right":{"node_type":"literal","type":"string","value":"db-svc"}},"right":{"node_type":"operator","operator":"matches","left":{"node_type":"field","name":"domain"},"right":{"node_type":"literal","type":"regex","value":"/example\\.com$/"}}},"right":{"node_type":"operator","operator":"matches","left":{"node_type":"field","name":"src.process.path"},"right":{"node_type":"literal","type":"regex","value":"|^/usr/bin/|"}}},"right":{"node_type":"operator","operator":"and","left":{"node_type":"operator","operator":"ne","left":{"node_type":"field","name":"process.uid"},"right":{"node_type":"literal","type":"int64","value":0}},"right":{"node_type":"operator","operator":"contains","left":{"node_type":"field","name":"tags"},"right":{"node_type":"literal","type":"string","value":"internal-svc"}}}},"right":{"node_type":"operator","operator":"and","left":{"node_type":"operator","operator":"le","left":{"node_type":"field","name":"destination.port"},"right":{"node_type":"literal","type":"int64","value":1023}},"right":{"node_type":"operator","operator":"eq","left":{"node_type":"field","name":"destination.ip"},"right":{"node_type":"literal","type":"cidr","value":"192.168.0.0/16"}}}}

// Default values
const DEFAULT_RULE_INPUT = `tags == 'db-svc'
OR domain matches /example\.com$/ -- any domain or subdomain of example.com
OR src.process.path matches |^/usr/bin/| -- patterns can be enclosed in |...| or /.../
OR (process.uid != 0 AND tags contains 'internal-svc') 
/* connections to LAN addresses over privileged ports */
OR (destination.port <= 1023 AND destination.ip == 192.168.0.0/16)`

const DEFAULT_DATA_JSON = JSON.stringify({
  "tags":   ["db-svc", "internal-vlan", "unprivileged-user"],
  "domain": "example.com",
  "process": {
    "uid":  1000,
    "path": "/usr/bin/some-other-process",
  },
  "port": 8080,
}, null, 2)

const DEFAULT_AST_JSON = JSON.stringify(sampleAST, null, 2)

// LocalStorage keys
const STORAGE_KEY_RULE = 'rulekit-rule-input'
const STORAGE_KEY_DATA = 'rulekit-data-json'
const STORAGE_KEY_AST = 'rulekit-ast-json'

// Load from localStorage or use defaults
const ruleInput = ref(localStorage.getItem(STORAGE_KEY_RULE) || DEFAULT_RULE_INPUT)
const dataJson = ref(localStorage.getItem(STORAGE_KEY_DATA) || DEFAULT_DATA_JSON)
const astJson = ref(localStorage.getItem(STORAGE_KEY_AST) || DEFAULT_AST_JSON)

// Reactive state
const result = ref<EvalResult | null>(null)
const ruleText = ref('')
const error = ref('')
const dataJsonError = ref('')
const ruleParseError = ref('')
const isParsing = ref(false)
const isAstExpanded = ref(false)

// CodeMirror extensions
const extensions = shallowRef([
  json(),
  oneDark
])

// Check if test data JSON is valid
const isDataJsonValid = computed(() => {
  try {
    JSON.parse(dataJson.value)
    dataJsonError.value = ''
    return true
  } catch (e: any) {
    dataJsonError.value = e.message
    return false
  }
})

// Evaluate rule
function evaluateRule() {
  error.value = ''
  result.value = null
  
  // Don't evaluate if data JSON is invalid
  if (!isDataJsonValid.value) {
    return
  }
  
  try {
    const ast = JSON.parse(astJson.value)
    const data = JSON.parse(dataJson.value)
    
    const rule = Rule.fromJSON(ast)
    ruleText.value = rule.toString()
    result.value = rule.eval(data)
  } catch (e: any) {
    error.value = e.message
  }
}

// Parse rule text via API
async function parseRule() {
  if (!ruleInput.value.trim()) {
    ruleParseError.value = 'Rule cannot be empty'
    return
  }

  isParsing.value = true
  ruleParseError.value = ''

  try {
    const response = await fetch('http://0.0.0.0:10002/devtools/api/rulekit/parse', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({ expr: ruleInput.value })
    })

    if (!response.ok) {
      const errorData = await response.json().catch(() => ({ error: 'Failed to parse response' }))
      throw new Error(errorData.error || `HTTP ${response.status}`)
    }

    const data = await response.json()
    
    if (data.error) {
      ruleParseError.value = data.error
      return
    }

    if (data.ast) {
      astJson.value = JSON.stringify(data.ast, null, 2)
      ruleParseError.value = ''
    } else {
      throw new Error('No AST in response')
    }
  } catch (e: any) {
    ruleParseError.value = e.message
  } finally {
    isParsing.value = false
  }
}

// Debounce parse function
const debouncedParse = debounce(parseRule, 20)

watch(ruleInput, () => {
  debouncedParse()
})


// Debounce helper
function debounce<T extends (...args: any[]) => any>(
  fn: T,
  delay: number
): (...args: Parameters<T>) => void {
  let timeoutId: ReturnType<typeof setTimeout> | null = null
  return (...args: Parameters<T>) => {
    if (timeoutId) clearTimeout(timeoutId)
    timeoutId = setTimeout(() => fn(...args), delay)
  }
}

// Auto-evaluate with debounce when inputs change
const debouncedEvaluate = debounce(evaluateRule, 500)

watch([astJson, dataJson], () => {
  debouncedEvaluate()
})

// Save to localStorage when values change
watch(ruleInput, (newVal) => {
  localStorage.setItem(STORAGE_KEY_RULE, newVal)
})

watch(dataJson, (newVal) => {
  localStorage.setItem(STORAGE_KEY_DATA, newVal)
})

watch(astJson, (newVal) => {
  localStorage.setItem(STORAGE_KEY_AST, newVal)
})

// Reset to defaults
function resetToDefaults() {
  ruleInput.value = DEFAULT_RULE_INPUT
  dataJson.value = DEFAULT_DATA_JSON
  astJson.value = DEFAULT_AST_JSON
  result.value = null
  error.value = ''
  ruleParseError.value = ''
  parseRule()
  evaluateRule()
}

// Evaluate on mount
onMounted(() => {
  parseRule()
  evaluateRule()
})
</script>

<template>
  <div>
    <div class="header-container">
      <h1>Rulekit.js</h1>
      <button @click="resetToDefaults" class="reset-button">🔄 Reset to Defaults</button>
    </div>

    <div class="inputs-container">
      <div class="card half-width">
        <h2>✍️ Write Your Rule</h2>
        <codemirror
          v-model="ruleInput"
          :extensions="extensions"
          :style="{ height: '100px', fontSize: '14px', marginBottom: '1em' }"
          :autofocus="false"
          :disabled="false"
          placeholder="Enter your rule here (e.g., ip in [1.2.3.4, 10.0.0.0/8])"
        />

        <div v-if="ruleParseError" class="json-error-message">
          <strong>⚠️ Parse Error:</strong> {{ ruleParseError }}
        </div>
        <div v-else-if="isParsing" class="parsing-indicator">
          Parsing...
        </div>
        <div v-else-if="ruleText" class="rule-expression">
          {{ ruleText }}
        </div>
      </div>

      <div class="card half-width">
        <h2>📦 Test Data (JSON)</h2>
        <div v-if="dataJsonError" class="json-error-message">
          <strong>⚠️ Invalid JSON:</strong> {{ dataJsonError }}
        </div>
        <codemirror
          v-model="dataJson"
          :extensions="extensions"
          :style="{ height: '400px', fontSize: '14px' }"
          :autofocus="false"
          :disabled="false"
          placeholder="Enter test data JSON..."
        />
      </div>
    </div>

    <div :class="['card', { 'card-collapsed': !isAstExpanded }]">
      <h2 @click="isAstExpanded = !isAstExpanded" class="collapsible-header">
        <span class="collapse-icon">{{ isAstExpanded ? '▼' : '▶' }}</span>
        🔧 AST Input (JSON from Go)
      </h2>
      <div v-if="isAstExpanded">
        <codemirror
          v-model="astJson"
          :extensions="extensions"
          :style="{ height: '400px', fontSize: '14px' }"
          :autofocus="false"
          :disabled="false"
          placeholder="Paste AST JSON here..."
        />
      </div>
    </div>

    <div v-if="result" class="card">
      <h2>✨ Result</h2>
      <div :class="['result', result.ok ? (result.value ? 'pass' : 'fail') : 'error']">
        <div v-if="result.ok">
          <strong>{{ result.value ? '✅ PASS' : '❌ FAIL' }}</strong>
          <div>Value: {{ result.value }}</div>
        </div>
        <div v-else>
          <strong>⚠️ ERROR</strong>
          <div>{{ result.error }}</div>
        </div>
      </div>
    </div>

    <div v-if="error" class="card">
      <div class="result error">
        <strong>⚠️ Parse Error</strong>
        <div>{{ error }}</div>
      </div>
    </div>
  </div>
</template>

