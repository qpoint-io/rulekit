<script setup lang="ts">
import { ref, computed, watch, shallowRef, onMounted, onUnmounted } from 'vue'
import { Codemirror } from 'vue-codemirror'
import { json } from '@codemirror/lang-json'
import { oneDark } from '@codemirror/theme-one-dark'
import { rulekit } from './rulekit-wasm'
import { ruleLanguage } from './ruleLanguage'

// Simple EvalResult type for display
interface EvalResult {
  ok: boolean
  value?: any
  error?: string
  evaluatedRule?: string
}

// Sample AST from Go
// TestEngineExample from rule_test.go
const sampleAST = {"node_type":"operator","operator":"or","left":{"node_type":"operator","operator":"or","left":{"node_type":"operator","operator":"or","left":{"node_type":"operator","operator":"or","left":{"node_type":"operator","operator":"eq","left":{"node_type":"field","name":"tags"},"right":{"node_type":"literal","type":"string","value":"db-svc"}},"right":{"node_type":"operator","operator":"matches","left":{"node_type":"field","name":"domain"},"right":{"node_type":"literal","type":"regex","value":"/example\\.com$/"}}},"right":{"node_type":"operator","operator":"matches","left":{"node_type":"field","name":"process.path"},"right":{"node_type":"literal","type":"regex","value":"|^/usr/bin/|"}}},"right":{"node_type":"operator","operator":"and","left":{"node_type":"operator","operator":"ne","left":{"node_type":"field","name":"process.uid"},"right":{"node_type":"literal","type":"int64","value":0}},"right":{"node_type":"operator","operator":"contains","left":{"node_type":"field","name":"tags"},"right":{"node_type":"literal","type":"string","value":"internal-svc"}}}},"right":{"node_type":"operator","operator":"and","left":{"node_type":"operator","operator":"le","left":{"node_type":"field","name":"destination.port"},"right":{"node_type":"literal","type":"int64","value":1023}},"right":{"node_type":"operator","operator":"eq","left":{"node_type":"field","name":"destination.ip"},"right":{"node_type":"literal","type":"cidr","value":"192.168.0.0/16"}}}}

// Default values
const DEFAULT_RULE_INPUT = `tags == 'db-svc'
OR domain matches /example\.com$/ -- any domain or subdomain of example.com
OR process.path matches |^/usr/bin/| -- patterns can be enclosed in |...| or /.../
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
const isWasmReady = ref(false)
const currentRuleHandle = ref<number | undefined>(undefined)

// Gif hover state and animation
const isGifHovered = ref(false)
const currentGifState = ref<'idle' | 'walking'>('idle')
const birdPosition = ref(0) // 0 = left, 100 = right (percentage)
const birdDirection = ref<'left' | 'right'>('right')
const lastWalkTime = ref(Date.now()) // Track last time bird entered walk state
const idleGif = '/gray_idle_8fps.gif'
const walkingGif = '/gray_walk_fast_8fps.gif'
const activeGif = '/gray_with_ball_8fps.gif'

// Animation state
let targetPosition = 0
let animationFrameId: number | null = null
const walkSpeed = 0.1 // percent per frame at 60fps

const currentGif = computed(() => {
  if (isGifHovered.value) {
    return activeGif
  }
  return currentGifState.value === 'idle' ? idleGif : walkingGif
})

const birdPositionStyle = computed(() => {
  return {
    transform: `translateX(${birdPosition.value}%)`,
    transition: 'none'
  }
})

const birdFlipStyle = computed(() => {
  return {
    transform: birdDirection.value === 'left' ? 'scaleX(-1)' : 'scaleX(1)',
    transition: 'none'
  }
})

// Animation loop
function animateBird() {
  if (isGifHovered.value || currentGifState.value !== 'walking') {
    return
  }

  const diff = targetPosition - birdPosition.value
  
  if (Math.abs(diff) > 0.5) {
    // Move toward target
    const step = Math.sign(diff) * Math.min(Math.abs(diff), walkSpeed)
    birdPosition.value += step
    animationFrameId = requestAnimationFrame(animateBird)
  } else {
    // Reached target
    birdPosition.value = targetPosition
    animationFrameId = null
  }
}

function startWalking(newTargetPosition: number) {
  targetPosition = newTargetPosition
  
  // Cancel existing animation
  if (animationFrameId !== null) {
    cancelAnimationFrame(animationFrameId)
    animationFrameId = null
  }
  
  // Start new animation
  animateBird()
}

// CodeMirror extensions
const ruleExtensions = shallowRef([
  ruleLanguage,
  oneDark
])

const jsonExtensions = shallowRef([
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

const showResetButton = computed(() => {
  return ruleInput.value !== DEFAULT_RULE_INPUT || dataJson.value !== DEFAULT_DATA_JSON
})

// Evaluate rule using WASM
async function evaluateRule() {
  error.value = ''
  result.value = null
  
  // Don't evaluate if WASM isn't ready or data JSON is invalid
  if (!isWasmReady.value || !isDataJsonValid.value) {
    return
  }
  
  // Don't evaluate if we don't have a valid rule handle
  if (currentRuleHandle.value === undefined) {
    return
  }
  
  try {
    const data = JSON.parse(dataJson.value)
    
    // Evaluate using WASM
    const evalResult = await rulekit.eval(currentRuleHandle.value, data)
    console.log('evalResult', evalResult)
    
    // Convert WASM result to our display format
    result.value = {
      ok: evalResult.ok,
      value: evalResult.value,
      error: evalResult.error,
      evaluatedRule: evalResult.evaluatedRule
    }
  } catch (e: any) {
    error.value = e.message
  }
}

// Parse rule text via WASM
async function parseRule() {
  if (!ruleInput.value.trim()) {
    ruleParseError.value = 'Rule cannot be empty'
    // Free old handle if it exists
    if (currentRuleHandle.value !== undefined) {
      await rulekit.free(currentRuleHandle.value)
      currentRuleHandle.value = undefined
    }
    return
  }

  if (!isWasmReady.value) {
    ruleParseError.value = 'WASM module not ready yet...'
    return
  }

  isParsing.value = true
  ruleParseError.value = ''

  try {
    // Free old handle before parsing new rule
    if (currentRuleHandle.value !== undefined) {
      await rulekit.free(currentRuleHandle.value)
      currentRuleHandle.value = undefined
    }

    // Parse using WASM
    const parsed = await rulekit.parse(ruleInput.value)
    
    if (parsed.error) {
      ruleParseError.value = parsed.error
      return
    }

    if (parsed.ast) {
      // Update AST display
      astJson.value = JSON.stringify(parsed.ast, null, 2)
      
      // Update rule text display
      if (parsed.ruleString) {
        ruleText.value = parsed.ruleString
      }
      
      // Store handle for evaluation
      currentRuleHandle.value = parsed.handle
      
      ruleParseError.value = ''
      
      // Auto-evaluate after successful parse
      await evaluateRule()
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
const debouncedParse = debounce(parseRule, 80)

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

// Auto-evaluate with debounce when data changes
const debouncedEvaluate = debounce(evaluateRule, 80)

watch(dataJson, () => {
  if (currentRuleHandle.value !== undefined) {
    debouncedEvaluate()
  }
})

// When AST is manually edited, need to re-parse to get a new handle
watch(astJson, () => {
  // Only trigger if user manually edited AST
  // (skip if it was just updated by parseRule)
  // This is a bit tricky - for now we'll just leave AST as display-only
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

// Watch for hover state changes
watch(isGifHovered, (hovered) => {
  if (hovered) {
    // Stop animation when hovered
    if (animationFrameId !== null) {
      cancelAnimationFrame(animationFrameId)
      animationFrameId = null
    }
  } else {
    // Resume animation if still walking
    if (currentGifState.value === 'walking' && Math.abs(targetPosition - birdPosition.value) > 0.5) {
      animateBird()
    }
  }
})

// Reset to defaults
async function resetToDefaults() {
  ruleInput.value = DEFAULT_RULE_INPUT
  dataJson.value = DEFAULT_DATA_JSON
  astJson.value = DEFAULT_AST_JSON
  result.value = null
  error.value = ''
  ruleParseError.value = ''
  await parseRule()
}

// Random state switching for gif
function randomGifStateSwitch() {
  if (!isGifHovered.value) {
    const timeSinceLastWalk = Date.now() - lastWalkTime.value
    const shouldForceWalk = timeSinceLastWalk > 6000 // Force walk if >6 seconds
    
    // Randomly switch between idle and walking, or force walking if it's been too long
    const newState = shouldForceWalk || Math.random() > 0.5 ? 'walking' : 'idle'
    
    if (newState === 'walking') {
      // Check current position and walk to the opposite edge
      const currentPos = birdPosition.value
      let newTarget: number
      
      // If in right half, walk left
      if (currentPos >= 90) {
        birdDirection.value = 'left'
        // Walk to somewhere in the left 30%
        newTarget = Math.random() * 30
      } else {
        // Otherwise walk right to the edge
        birdDirection.value = 'right'
        // Walk to somewhere in the right 50%-85%
        newTarget = 50 + Math.random() * 35
      }
      
      // Only actually walk if the distance is significant (more than 20%)
      if (Math.abs(newTarget - currentPos) > 20) {
        // Set state BEFORE starting animation so animateBird check passes
        currentGifState.value = newState
        lastWalkTime.value = Date.now() // Update last walk time
        startWalking(newTarget)
      } else {
        // Distance too small, try again soon
        setTimeout(randomGifStateSwitch, 200)
        return
      }
    } else {
      // Stop animation when idle
      if (animationFrameId !== null) {
        cancelAnimationFrame(animationFrameId)
        animationFrameId = null
      }
      currentGifState.value = newState
    }
  }
  
  // Schedule next switch with random interval (1-2 seconds)
  const nextInterval = 1000 + Math.random() * 1000
  setTimeout(randomGifStateSwitch, nextInterval)
}

// Preload gifs and initialize WASM
onMounted(async () => {
  // Preload all gifs
  const img1 = new Image()
  const img2 = new Image()
  const img3 = new Image()
  img1.src = idleGif
  img2.src = walkingGif
  img3.src = activeGif
  
  // Start random state switching
  randomGifStateSwitch()
  
  // Wait for WASM to be ready
  try {
    await rulekit.waitReady()
    isWasmReady.value = true
    console.log('✅ Rulekit WASM ready!')
    
    // Now parse and evaluate
    await parseRule()
  } catch (e: any) {
    console.error('❌ Failed to initialize WASM:', e)
    ruleParseError.value = 'Failed to load WASM module: ' + e.message
  }
})

// Cleanup on unmount
onUnmounted(async () => {
  if (animationFrameId !== null) {
    cancelAnimationFrame(animationFrameId)
    animationFrameId = null
  }
  
  // Free rule handle
  if (currentRuleHandle.value !== undefined) {
    await rulekit.free(currentRuleHandle.value)
    currentRuleHandle.value = undefined
  }
})
</script>

<template>
  <div>
    <!-- Animated gif above header -->
    <div class="gif-container">
      <div 
        class="bird-position" 
        :style="birdPositionStyle"
      >
        <img 
          :src="currentGif" 
          :style="birdFlipStyle"
          alt="Animated character"
          class="corner-gif"
          @mouseenter="isGifHovered = true" 
          @mouseleave="isGifHovered = false"
        />
      </div>
    </div>

    <div class="header-container">
      <h1>Rulekit.js</h1>
      <button v-if="showResetButton" @click="resetToDefaults" class="reset-button">Reset to Example Data</button>
    </div>

    <div class="inputs-container">
      <div class="card half-width">
        <h2><span class="icon">▶</span> Write Your Rule</h2>
        <codemirror
          v-model="ruleInput"
          :extensions="ruleExtensions"
          :style="{ fontSize: '14px' }"
          :autofocus="false"
          :disabled="false"
          placeholder="Enter your rule here (e.g., ip in [1.2.3.4, 10.0.0.0/8])"
        />
      </div>

      <div class="card half-width">
        <h2><span class="icon">▣</span> Test Data (JSON)</h2>
        <div v-if="dataJsonError" class="json-error-message">
          <strong>Invalid JSON:</strong> {{ dataJsonError }}
        </div>
        <codemirror
          v-model="dataJson"
          :extensions="jsonExtensions"
          :style="{ fontSize: '14px' }"
          :autofocus="false"
          :disabled="false"
          placeholder="Enter test data JSON..."
        />
      </div>
    </div>

    <div v-if="ruleParseError" class="card result error">
      <h2>
        <span class="result-inline error"><span style="font-size: 1.5em;">🮽</span> Invalid Rule</span>
      </h2>
      <pre style="margin-top:0" class="rule-expression">{{ ruleParseError }}</pre>
    </div>

    <div v-if="!ruleParseError && result" class="card" :class="['result', result.ok ? (result.value ? 'pass' : 'fail') : 'error']">
      <h2><span v-if="result" :class="['result-inline', result.ok ? (result.value ? 'pass' : 'fail') : 'error']">
        <span style="font-size: 1.5em;">{{ result.ok ? (result.value ? '🮱' : '🮽') : '🯄' }}</span> {{ result.value ? 'PASS' : 'FAIL' }}
      </span></h2>
      
      <div v-if="result.ok" style="margin-bottom: 1em;">Value
        <div class="rule-expression">{{ result.value || (result.value === false ? 'false' : JSON.stringify(result.value)) }}</div>
      </div>
      <div v-else>Error
        <pre style="margin-top:0" class="rule-expression">{{ result.error }}</pre>
      </div>

        <div>Evaluated Rule
          <div class="rule-expression">{{ result.evaluatedRule }}</div>
        </div>
    </div>

    <div v-if="error" class="card">
      <div class="result error">
        <strong><span class="icon">!</span> Parse Error</strong>
        <div>{{ error }}</div>
      </div>
    </div>

    <div :class="['card', { 'card-collapsed': !isAstExpanded }]">
      <h2 @click="isAstExpanded = !isAstExpanded" class="collapsible-header">
        <span class="collapse-icon icon">{{ isAstExpanded ? '▼' : '▶' }}</span>
        AST Input (JSON from Go)
      </h2>
      <div v-if="isAstExpanded">
        <codemirror
          v-model="astJson"
          :extensions="jsonExtensions"
          :style="{ height: '400px', fontSize: '14px' }"
          :autofocus="false"
          :disabled="false"
          placeholder="Paste AST JSON here..."
        />
      </div>
    </div>
  </div>
</template>

