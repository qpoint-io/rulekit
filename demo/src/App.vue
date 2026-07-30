<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue'
import AstNodeView from './components/AstNode.vue'
import SelectionBridge from './components/SelectionBridge.vue'
import TraceNodeView from './components/TraceNode.vue'
import {
  deleteRuleNode,
  evalRule,
  flattenAST,
  formatRule,
  loadRulekit,
  nodeRef,
  parseRule,
  rewriteRule,
  type AstNode,
  type EvalResponse,
  type ParseResponse,
  type Token,
} from './lib/rulekit'

const exampleSource = `-- restrict database connections
(
  dst.port in [3306, 5432, 27017, 6379]
  and src.pod.namespace in ["api", "backend"]
)
-- allow essential services
or dst.domain in ["registry.k8s.io", "docker.io"]`

const inputPresets = {
  pass: `{
  "dst": {
    "domain": "registry.k8s.io",
    "port": 3306
  },
  "src": {
    "pod": { "namespace": "api" }
  }
}`,
  fail: `{
  "dst": {
    "domain": "example.com",
    "port": 8080
  },
  "src": {
    "pod": { "namespace": "monitoring" }
  }
}`,
  missing: `{
  "dst": {
    "port": 3306
  },
  "src": {
    "pod": { "namespace": "api" }
  }
}`,
  diag: `{
  "dst": {
    "domain": 42,
    "port": "three thousand"
  },
  "src": {
    "pod": { "namespace": ["api"] }
  }
}`,
}

const source = ref(exampleSource)
const inputJSON = ref(inputPresets.pass)
const parseResult = ref<ParseResponse>({ ok: false })
const lastValid = ref<ParseResponse>({ ok: false })
const evalResult = ref<EvalResponse | null>(null)
const selectedId = ref('root')
const hoveredId = ref<string | undefined>()
const editorMode = ref<'nested' | 'chips' | 'graph'>('nested')
const density = ref<'comfortable' | 'compact' | 'dense'>('compact')
const selectionSync = ref(true)
const syncHintOpen = ref(true)
const wasmError = ref('')
const rewriteError = ref('')
const draft = ref('')
const textarea = ref<HTMLTextAreaElement | null>(null)

const currentAST = computed(() => parseResult.value.ast || lastValid.value.ast)
const currentTokens = computed(() => parseResult.value.tokens || lastValid.value.tokens || [])
const nodes = computed(() => flattenAST(currentAST.value))
const selectedNode = computed(() => nodes.value.find((node) => node.id === selectedId.value))
const activeNodeId = computed(() => hoveredId.value || selectedId.value)
const evalStatus = computed(() => evalResult.value?.status || 'unknown')

const resultClass = computed(() => {
  if (evalStatus.value === 'passed') return 'rk-panel--ok'
  if (evalStatus.value === 'failed') return 'rk-panel--no'
  if (evalStatus.value === 'missing') return 'rk-panel--missing'
  if (evalStatus.value === 'error') return 'rk-panel--error'
  return ''
})

const editableKind = computed(() => {
  const node = selectedNode.value
  if (!node) return 'none'
  if (node.kind === 'binary') return 'operator'
  if (node.kind === 'path') return 'path'
  if (node.kind === 'literal') return 'literal'
  return 'none'
})

const canDelete = computed(() => {
  const node = selectedNode.value
  if (!node || node.id === 'root') return false
  const parentId = node.id.split('.').slice(0, -1).join('.')
  const parent = nodes.value.find((item) => item.id === parentId)
  if (!parent) return false
  if (parent.kind === 'binary') return parent.operator === 'and' || parent.operator === 'or'
  if (parent.kind === 'unary') return true
  if (parent.kind === 'array') return (parent.children?.length || 0) > 1
  return false
})

watch(selectedNode, (node) => {
  if (!node) {
    draft.value = ''
    return
  }
  if (node.kind === 'binary') draft.value = node.operator || ''
  else if (node.kind === 'path') draft.value = node.path || node.text
  else if (node.kind === 'literal') draft.value = node.raw || node.text
  else draft.value = node.text
}, { immediate: true })

watch(source, () => {
  window.clearTimeout(parseTimer)
  parseTimer = window.setTimeout(() => void refreshParse(), 120)
})

watch([source, inputJSON], () => {
  window.clearTimeout(evalTimer)
  evalTimer = window.setTimeout(() => void refreshEval(), 180)
})

let parseTimer = 0
let evalTimer = 0

onMounted(async () => {
  try {
    await loadRulekit()
    await refreshParse()
    await refreshEval()
  } catch (err) {
    wasmError.value = err instanceof Error ? err.message : String(err)
  }
})

async function refreshParse() {
  parseResult.value = await parseRule(source.value)
  if (parseResult.value.ok) {
    lastValid.value = parseResult.value
    rewriteError.value = ''
    if (!selectedNode.value) selectedId.value = parseResult.value.ast?.id || 'root'
  }
}

async function refreshEval() {
  if (!parseResult.value.ok) return
  evalResult.value = await evalRule(source.value, inputJSON.value)
}

async function applyFormat(mode: 'compact' | 'multiline') {
  const res = await formatRule(source.value, mode)
  if (!res.ok || !res.source) {
    rewriteError.value = res.error || 'format failed'
    return
  }
  source.value = res.source
  await nextTick()
  await refreshParse()
  await refreshEval()
}

async function applyRewrite() {
  const node = selectedNode.value
  if (!node || editableKind.value === 'none') return

  const replacement = editableKind.value === 'path' ? draft.value.trim() : draft.value
  const res = await rewriteRule(source.value, {
    target: nodeRef(node),
    replacement,
    kind: editableKind.value === 'operator' ? 'operator' : 'node',
    mode: 'source',
  })
  if (!res.ok || !res.source) {
    rewriteError.value = res.error || 'rewrite failed'
    return
  }
  source.value = res.source
  parseResult.value = res
  lastValid.value = res
  rewriteError.value = ''
  await refreshEval()
}

async function applyDelete() {
  const node = selectedNode.value
  if (!node) return
  const res = await deleteRuleNode(source.value, nodeRef(node))
  if (!res.ok || !res.source) {
    rewriteError.value = res.error || 'delete failed'
    return
  }
  source.value = res.source
  parseResult.value = res
  lastValid.value = res
  selectedId.value = res.ast?.id || 'root'
  rewriteError.value = ''
  await refreshEval()
}

function selectNode(node: AstNode) {
  selectedId.value = node.id
  textarea.value?.focus()
  textarea.value?.setSelectionRange(node.span.start, node.span.end)
}

function selectTrace(trace: { span?: { start: number; end: number } }) {
  if (!trace.span) return
  const match = nodes.value.find((node) => node.span.start === trace.span?.start && node.span.end === trace.span?.end)
  if (match) selectNode(match)
}

function hoverTrace(trace?: { span?: { start: number; end: number } }) {
  if (!trace?.span) {
    hoveredId.value = undefined
    return
  }
  const match = nodes.value.find((node) => node.span.start === trace.span?.start && node.span.end === trace.span?.end)
  hoveredId.value = match?.id
}

function nodeIdForToken(token: Token) {
  const containing = nodes.value
    .filter((node) => node.span.start <= token.span.start && node.span.end >= token.span.end)
    .sort((a, b) => (a.span.end - a.span.start) - (b.span.end - b.span.start))[0]
  return containing?.id
}

function selectFromText() {
  const el = textarea.value
  if (!el) return
  const start = Math.min(el.selectionStart, el.selectionEnd)
  const end = Math.max(el.selectionStart, el.selectionEnd)
  const pointEnd = start === end ? start + 1 : end
  const match = nodes.value
    .filter((node) => node.span.start <= start && node.span.end >= pointEnd)
    .sort((a, b) => (a.span.end - a.span.start) - (b.span.end - b.span.start))[0]
  if (match) selectedId.value = match.id
}

function tokenClass(token: Token) {
  const active = nodes.value.find((node) => node.id === activeNodeId.value)
  const selected = selectionSync.value && active
    ? token.span.start >= active.span.start && token.span.end <= active.span.end
    : false
  return [`tok-${token.role}`, selected ? 'is-selected' : '']
}

const tokenLines = computed(() => {
  const lines: Array<Array<Token & { nodeId?: string }>> = []
  for (const token of currentTokens.value) {
    const lineIndex = Math.max(0, token.span.startLine - 1)
    lines[lineIndex] ||= []
    lines[lineIndex].push({ ...token, nodeId: nodeIdForToken(token) })
  }
  return lines
})

function statusLabel() {
  if (!evalResult.value) return 'WAIT'
  if (evalResult.value.error) return 'ERROR'
  if (evalResult.value.status === 'passed') return 'PASS'
  if (evalResult.value.status === 'failed') return 'FAIL'
  if (evalResult.value.status === 'missing') return 'MISSING'
  return 'UNKNOWN'
}

function valueText(value: unknown) {
  return value === undefined ? 'undefined' : JSON.stringify(value)
}
</script>

<template>
  <main class="rk-app" :class="`rk-app--${density}`">
    <header class="rk-header">
      <div class="rk-wordmark-wrap">
        <img src="/assets/mascot.png" class="rk-mascot" alt="rulekit mascot" />
        <h1 class="rk-wordmark">rulekit<span class="rk-v2">V2</span></h1>
        <p class="rk-tagline">editor playground · text ↔ ast ↔ trace</p>
      </div>
      <div class="rk-header-meta">
        <div class="row"><span class="dot dot--blue" />engine <b>go · wasm</b></div>
        <div class="row"><span class="dot" />last eval <b>{{ statusLabel() }}</b></div>
        <div class="row">
          density
          <button class="rk-btn rk-btn--ghost" :class="{ 'rk-btn--active': density === 'comfortable' }" @click="density = 'comfortable'">roomy</button>
          <button class="rk-btn rk-btn--ghost" :class="{ 'rk-btn--active': density === 'compact' }" @click="density = 'compact'">compact</button>
          <button class="rk-btn rk-btn--ghost" :class="{ 'rk-btn--active': density === 'dense' }" @click="density = 'dense'">dense</button>
        </div>
        <div class="row">
          sync
          <button class="rk-btn rk-btn--ghost" :class="{ 'rk-btn--active': selectionSync }" @click="selectionSync = !selectionSync">{{ selectionSync ? 'on' : 'off' }}</button>
        </div>
      </div>
    </header>

    <div v-if="syncHintOpen" class="rk-selsync-hint">
      <span class="glyph" :class="{ on: selectionSync }" />
      <span><b>selection sync {{ selectionSync ? 'on' : 'off' }}</b><span>hover or click a token / chip / trace node</span></span>
      <button class="rk-btn rk-btn--ghost rk-btn--icon" @click="syncHintOpen = false">x</button>
    </div>
    <button v-else class="rk-selsync-pill" @click="syncHintOpen = true"><span class="glyph" :class="{ on: selectionSync }" />sync</button>

    <div v-if="wasmError" class="rk-panel rk-panel--error rk-banner">{{ wasmError }}</div>

    <div class="rk-stack">
      <section class="rk-panel rk-panel--split">
        <div class="rk-panel-head rk-panel-head--main">
          <h2 class="rk-panel-title"><span class="rk-step">1</span>Rule <span class="chev">/ edit source or ast</span></h2>
          <div class="rk-panel-actions">
            <button class="rk-btn" @click="applyFormat('compact')">compact</button>
            <button class="rk-btn" @click="applyFormat('multiline')">multiline</button>
            <button class="rk-btn rk-btn--ghost" @click="source = exampleSource">reset</button>
          </div>
        </div>

        <div class="rk-split">
          <div class="rk-split-col">
            <div class="rk-split-head">
              <span class="rk-split-tag">text</span>
              <span v-if="parseResult.ok" class="rk-label">parsed</span>
              <span v-else class="rk-label rk-label--bad">parse error</span>
            </div>
            <div class="rk-editor-wrap">
              <textarea
                ref="textarea"
                v-model="source"
                class="rk-source"
                spellcheck="false"
                @select="selectFromText"
                @keyup="selectFromText"
                @click="selectFromText"
              />
              <div class="rk-token-strip" aria-hidden="true">
                <span v-for="(token, index) in currentTokens" :key="index" class="tok" :class="tokenClass(token)">{{ token.raw }}</span>
              </div>
            </div>
            <pre v-if="parseResult.error" class="rk-error">{{ parseResult.error }}</pre>
            <div class="rk-source-map" data-screen-label="source-map">
              <div v-for="(line, lineIndex) in tokenLines" :key="lineIndex" class="rk-source-line">
                <span class="rk-gutter">{{ lineIndex + 1 }}</span>
                <span class="rk-source-line-content">
                  <span
                    v-for="(token, tokenIndex) in line"
                    :key="tokenIndex"
                    class="tok"
                    :class="tokenClass(token)"
                    :data-node-id="token.nodeId"
                    @click="token.nodeId && (selectedId = token.nodeId)"
                    @mouseenter="hoveredId = token.nodeId"
                    @mouseleave="hoveredId = undefined"
                  >{{ token.raw }}</span>
                </span>
              </div>
            </div>
          </div>

          <div class="rk-split-col">
            <div class="rk-split-head">
              <span class="rk-split-tag">visual · {{ editorMode }}</span>
              <span class="rk-panel-actions">
                <button class="rk-btn rk-btn--ghost" :class="{ 'rk-btn--active': editorMode === 'nested' }" @click="editorMode = 'nested'">nested</button>
                <button class="rk-btn rk-btn--ghost" :class="{ 'rk-btn--active': editorMode === 'chips' }" @click="editorMode = 'chips'">chips</button>
                <button class="rk-btn rk-btn--ghost" :class="{ 'rk-btn--active': editorMode === 'graph' }" @click="editorMode = 'graph'">graph</button>
              </span>
            </div>
            <div class="rk-visual" data-screen-label="visual-ast">
              <AstNodeView
                v-if="currentAST"
                :node="currentAST"
                :selected-id="selectionSync ? activeNodeId : undefined"
                :mode="editorMode"
                @select="selectNode"
                @hover="hoveredId = $event?.id"
              />
            </div>

            <div class="rk-edit-card">
              <div class="rk-edit-title">selected · <b>{{ selectedNode?.kind || 'none' }}</b></div>
              <div class="rk-edit-meta">{{ selectedNode?.text || 'click a node to edit or delete safe nodes' }}</div>
              <template v-if="selectedNode && editableKind !== 'none'">
                <select v-if="editableKind === 'operator'" v-model="draft" class="rk-input">
                  <option v-for="op in ['and', 'or', '==', '!=', '>', '>=', '<', '<=', 'contains', 'matches', 'in']" :key="op" :value="op">{{ op }}</option>
                </select>
                <input v-else v-model="draft" class="rk-input" />
                <button class="rk-btn" @click="applyRewrite">rewrite</button>
              </template>
              <button class="rk-btn rk-btn--no" :disabled="!canDelete" @click="applyDelete">delete safe node</button>
              <pre v-if="rewriteError" class="rk-error">{{ rewriteError }}</pre>
            </div>
          </div>
        </div>
      </section>

      <section class="rk-panel">
        <div class="rk-panel-head rk-panel-head--main">
          <h2 class="rk-panel-title"><span class="rk-step">2</span>Input <span class="chev">/ json evaluated against rule</span></h2>
          <div class="rk-panel-actions">
            <button class="rk-btn" @click="inputJSON = inputPresets.pass">pass</button>
            <button class="rk-btn rk-btn--ghost" @click="inputJSON = inputPresets.fail">fail</button>
            <button class="rk-btn rk-btn--ghost" @click="inputJSON = inputPresets.missing">missing</button>
            <button class="rk-btn rk-btn--ghost" @click="inputJSON = inputPresets.diag">diagnostic</button>
          </div>
        </div>
        <textarea v-model="inputJSON" class="rk-json" spellcheck="false" />
      </section>

      <section class="rk-panel" :class="resultClass">
        <div class="rk-panel-head rk-panel-head--main">
          <h2 class="rk-panel-title"><span class="rk-step" :class="`rk-step--${evalStatus}`">3</span>Result <span class="chev">/ trace from Go evaluator</span></h2>
          <button class="rk-btn" @click="refreshEval">evaluate</button>
        </div>
        <div class="rk-result-grid">
          <div class="rk-result-card">
            <div class="rk-marquee">{{ statusLabel() }}</div>
            <div class="rk-return">return value <b>{{ valueText(evalResult?.value) }}</b></div>
            <div v-if="evalResult?.missingFields?.length" class="rk-trace-note">missing: {{ evalResult.missingFields.join(', ') }}</div>
            <div v-if="evalResult?.error" class="rk-error">{{ evalResult.error }}</div>
          </div>
          <div class="rk-trace">
            <TraceNodeView
              v-if="evalResult?.trace"
              :trace="evalResult.trace"
              :selected-id="selectedNode ? `${selectedNode.span.start}:${selectedNode.span.end}` : undefined"
              @select="selectTrace"
              @hover="hoverTrace"
            />
          </div>
        </div>
      </section>
    </div>

    <SelectionBridge :selected-id="activeNodeId" :enabled="selectionSync" />
  </main>
</template>
