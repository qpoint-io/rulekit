<script setup lang="ts">
import type { TraceNode } from '../lib/rulekit'

defineOptions({ name: 'TraceNodeView' })

defineProps<{
  trace: TraceNode
  selectedId?: string
}>()

const emit = defineEmits<{
  select: [trace: TraceNode]
  hover: [trace?: TraceNode]
}>()
</script>

<template>
  <div
    class="rk-trace-node"
    :class="[`rk-trace-node--${trace.status || 'unknown'}`, { 'is-selected': selectedId && trace.span && selectedId === `${trace.span.start}:${trace.span.end}` }]"
    @click.stop="emit('select', trace)"
    @mouseenter="emit('hover', trace)"
    @mouseleave="emit('hover')"
  >
    <div class="rk-trace-head">
      <span class="rk-trace-status">{{ trace.status || 'unknown' }}</span>
      <span class="rk-trace-expr">{{ trace.expr || trace.kind || '<node>' }}</span>
      <span v-if="trace.pruned" class="rk-trace-pill">pruned</span>
      <span v-if="trace.value !== undefined" class="rk-trace-value">value: {{ JSON.stringify(trace.value) }}</span>
    </div>
    <div v-if="trace.missingFields?.length" class="rk-trace-note">
      missing: {{ trace.missingFields.join(', ') }}
    </div>
    <div v-if="trace.diagnostics?.length" class="rk-trace-note">
      {{ trace.diagnostics.map((d) => d.Message).join(' · ') }}
    </div>
    <div v-if="trace.children?.length" class="rk-trace-children">
      <TraceNodeView
        v-for="(child, index) in trace.children"
        :key="index"
        :trace="child"
        :selected-id="selectedId"
        @select="emit('select', $event)"
        @hover="emit('hover', $event)"
      />
    </div>
  </div>
</template>
