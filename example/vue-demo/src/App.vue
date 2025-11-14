<script setup lang="ts">
import { ref, computed, watch, shallowRef, onMounted, onUnmounted } from 'vue'
import { Codemirror } from 'vue-codemirror'
import { json } from '@codemirror/lang-json'
import { abyss } from '@fsegurai/codemirror-theme-abyss'
import { rulekit } from './rulekit-wasm'
import { ruleLanguage } from './ruleLanguage'

// Simple EvalResult type for display
interface EvalResult {
  ok: boolean
  value?: any
  error?: string
  evaluatedRule?: string
  passFailErrStr?: string
}


// Default values
const DEFAULT_RULE_INPUT = `-- restrict database connections
(
  dst.port in [
    3306,  -- MySQL
    5432,  -- PostgreSQL
    27017, -- MongoDB
    6379   -- Redis
  ]
  and (
    src.pod.namespace in ["api", "backend"]
    or starts_with(src.pod.namespace, "kube-")
  )
)

-- allow essential services
or dst.domain in ["registry.k8s.io", "k8s.gcr.io", "gcr.io", "docker.io"]`

const DEFAULT_DATA_JSON_PASS = JSON.stringify({
  "dst": {
    "domain": "registry.k8s.io",
    "port": 3306,
  }
}, null, 2)

const DEFAULT_DATA_JSON_FAIL = JSON.stringify({
  "dst": {
    "domain": "example.com",
    "port": 3306,
  },
  "src": {
    "ip": "192.168.0.1",
    "port": 8080,
    "pod": {
      "namespace": "monitoring",
    },
  },
}, null, 2)

const DEFAULT_DATA_JSON = DEFAULT_DATA_JSON_PASS

// LocalStorage keys
const STORAGE_KEY_RULE = 'rulekit-rule-input'
const STORAGE_KEY_DATA = 'rulekit-data-json'
const STORAGE_KEY_AST = 'rulekit-ast-json'

// Load from localStorage or use defaults
const ruleInput = ref(localStorage.getItem(STORAGE_KEY_RULE) || DEFAULT_RULE_INPUT)
const dataJson = ref(localStorage.getItem(STORAGE_KEY_DATA) || DEFAULT_DATA_JSON)
const astJson = ref(localStorage.getItem(STORAGE_KEY_AST) || '')

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
  abyss
])

const jsonExtensions = shallowRef([
  json(),
  abyss
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
  return ruleInput.value !== DEFAULT_RULE_INPUT
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
      evaluatedRule: evalResult.evaluatedRule,
      passFailErrStr: evalResult.ok ? (evalResult.pass ? 'pass' : 'fail') : 'error'
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
  await parseRule()
}

// Set pass data
async function setPassData() {
  dataJson.value = DEFAULT_DATA_JSON_PASS
  await evaluateRule()
}

// Set fail data
async function setFailData() {
  dataJson.value = DEFAULT_DATA_JSON_FAIL
  await evaluateRule()
}

// Click on bird
function clickOnBird() {
  birdDirection.value = birdDirection.value === 'left' ? 'right' : 'left'
  cancelAnimation()
  targetPosition = birdDirection.value === 'left' ? 0 : 100
  animateBird()
}

function cancelAnimation() {
  if (animationFrameId !== null) {
    cancelAnimationFrame(animationFrameId)
    animationFrameId = null
    currentGifState.value = 'idle'
  }
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
      cancelAnimation()
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
    <div class="header-container">
      <h1>Rulekit</h1>
      <div class="gif-container">
        <div class="bird-position" :style="birdPositionStyle">
          <img
            draggable="false"
            :src="currentGif" 
            :style="birdFlipStyle"
            alt="Mickey"
            class="corner-gif"
            @click="clickOnBird"
            @mouseenter="isGifHovered = true" 
            @mouseleave="isGifHovered = false"
          />
        </div>
      </div>
    </div>

    <div class="inputs-container">
      <div class="card half-width">
        <a class="docs-link" href="https://github.com/qpoint-io/rulekit" target="_blank" title="Documentation">?</a>
        <div class="card-header-with-button">
          <h2>Rule</h2>
          <button v-if="showResetButton" @click="resetToDefaults" class="inline-button">Reset to Example Rule</button>
        </div>
        <codemirror
          v-model="ruleInput"
          :extensions="ruleExtensions"
          :style="{ fontSize: '14px', cursor: 'text', maxHeight: '500px' }"
          :autofocus="false"
          :disabled="false"
          placeholder="Enter your rule here (e.g., ip in [1.2.3.4, 10.0.0.0/8])"
        />
      </div>

      <div class="card half-width">
        <div class="card-header-with-button">
          <h2>Input</h2>
          <div class="button-group">
            <span class="inline-button">Load example data</span>
            <button @click="setPassData" class="inline-button pass">Pass</button>
            <button @click="setFailData" class="inline-button fail">Fail</button>
          </div>
        </div>
        <div v-if="dataJsonError" class="json-error-message">
          <strong>Invalid JSON</strong> <pre style="margin:0" class="rule-expression error">{{ dataJsonError }}</pre>
        </div>
        <codemirror
          v-model="dataJson"
          :extensions="jsonExtensions"
          :style="{ fontSize: '14px', cursor: 'text', maxHeight: '500px' }"
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
      <pre style="margin-top:0" class="rule-expression fail">{{ ruleParseError }}</pre>
    </div>

    <div v-if="!ruleParseError && result" class="card" :class="['result', result.passFailErrStr]">
      <h2><span v-if="result" :class="['result-inline', result.passFailErrStr]">
        <span style="font-size: 1.5em;">{{ result.ok ? (result.value ? '🮱' : '🮽') : '🯄' }}</span> {{ result.passFailErrStr?.toUpperCase() }}
      </span></h2>
      
      <div v-if="result.ok" style="margin-bottom: 1em;">Return Value
        <div class="rule-expression" :class="[result.passFailErrStr]">{{ result.value || (result.value === false ? 'false' : JSON.stringify(result.value)) }}</div>
      </div>
      <div v-else>Error
        <pre style="margin-top:0" class="rule-expression error">{{ result.error }}</pre>
      </div>

        <div>Evaluated Rule
          
          <codemirror
            v-model="result.evaluatedRule"
            :extensions="ruleExtensions"
            :style="{ fontSize: '14px', cursor: 'text', maxHeight: '500px' }"
            :autofocus="false"
            :disabled="true"
            placeholder="Evaluated Rule"
            :lineNumbers="false"
          />
        </div>
    </div>

    <div v-if="error" class="card">
      <div class="result error">
        <strong><span class="icon">!</span> Parse Error</strong>
        <div>{{ error }}</div>
      </div>
    </div>

    <div v-if="false" :class="['card', { 'card-collapsed': !isAstExpanded }]">
      <h2 @click="isAstExpanded = !isAstExpanded" class="collapsible-header">
        <span class="collapse-icon icon">{{ isAstExpanded ? '▼' : '▶' }}</span>
        AST Input (JSON from Go)
      </h2>
      <div v-if="isAstExpanded">
        <codemirror
          v-model="astJson"
          :extensions="jsonExtensions"
          :style="{ height: '400px', fontSize: '14px', cursor: 'text' }"
          :autofocus="false"
          :disabled="false"
          placeholder="Paste AST JSON here..."
        />
      </div>
    </div>
  </div>
</template>

