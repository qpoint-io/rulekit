<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'

const props = defineProps<{
  selectedId?: string
  enabled: boolean
}>()

type Coords = { x1: number; y1: number; x2: number; y2: number }

const coords = ref<Coords | null>(null)
const width = ref(0)
const height = ref(0)
let timer = 0

function compute() {
  if (!props.enabled || !props.selectedId) {
    coords.value = null
    return
  }
  width.value = window.innerWidth
  height.value = window.innerHeight
  const textEl = document.querySelector(`[data-screen-label="source-map"] [data-node-id="${props.selectedId}"]`)
  const visualEl = document.querySelector(`[data-screen-label="visual-ast"] [data-node-id="${props.selectedId}"]`)
  if (!textEl || !visualEl) {
    coords.value = null
    return
  }
  const a = textEl.getBoundingClientRect()
  const b = visualEl.getBoundingClientRect()
  const next = {
    x1: a.right + 2,
    y1: a.top + a.height / 2,
    x2: b.left - 2,
    y2: b.top + b.height / 2,
  }
  if (next.y1 < -40 || next.y1 > window.innerHeight + 40 || next.y2 < -40 || next.y2 > window.innerHeight + 40) {
    coords.value = null
    return
  }
  coords.value = next
}

function schedule() {
  void nextTick(compute)
}

onMounted(() => {
  schedule()
  window.addEventListener('scroll', compute, true)
  window.addEventListener('resize', compute)
  timer = window.setInterval(compute, 250)
})

onBeforeUnmount(() => {
  window.removeEventListener('scroll', compute, true)
  window.removeEventListener('resize', compute)
  window.clearInterval(timer)
})

watch(() => [props.selectedId, props.enabled], schedule)
</script>

<template>
  <svg v-if="coords" class="rk-bridge-layer" :width="width" :height="height" :viewBox="`0 0 ${width} ${height}`">
    <path
      class="rk-bridge-line"
      :d="`M ${coords.x1} ${coords.y1} C ${(coords.x1 + coords.x2) / 2} ${coords.y1}, ${(coords.x1 + coords.x2) / 2} ${coords.y2}, ${coords.x2} ${coords.y2}`"
    />
    <circle :cx="coords.x1" :cy="coords.y1" r="3" fill="var(--blue)" />
    <circle :cx="coords.x2" :cy="coords.y2" r="3" fill="var(--blue)" />
  </svg>
</template>
