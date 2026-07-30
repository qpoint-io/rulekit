<script setup lang="ts">
import type { AstNode } from '../lib/rulekit'

defineOptions({ name: 'AstNodeView' })

const props = defineProps<{
  node: AstNode
  selectedId?: string
  mode: 'nested' | 'chips' | 'graph'
}>()

const emit = defineEmits<{
  select: [node: AstNode]
  hover: [node?: AstNode]
}>()

function title(node: AstNode): string {
  if (node.kind === 'binary') return node.operator || node.raw || 'binary'
  if (node.kind === 'literal') return node.raw || node.text
  if (node.kind === 'path') return node.path || node.text
  return node.kind
}

function chipClass(node: AstNode): string {
  if (node.kind === 'path') return 'rk-chip--id'
  if (node.kind === 'literal' && /^[-+]?\d/.test(node.raw || node.text)) return 'rk-chip--num'
  if (node.kind === 'literal') return 'rk-chip--str'
  if (node.kind === 'binary') return 'rk-chip--op'
  if (node.kind === 'array') return 'rk-chip--array'
  return ''
}
</script>

<template>
  <div v-if="mode === 'graph'" class="rk-graph-node">
    <button
      class="rk-graph-self"
      :class="[`rk-graph-self--${node.kind}`, { 'is-selected': props.selectedId === node.id }]"
      type="button"
      :data-node-id="node.id"
      @click.stop="emit('select', node)"
      @mouseenter="emit('hover', node)"
      @mouseleave="emit('hover')"
    >
      <span class="kind">{{ node.kind }}</span>
      <span class="op">{{ title(node) }}</span>
      <span v-if="node.children?.length" class="branch-count">{{ node.children.length }}</span>
    </button>
    <div v-if="node.children?.length" class="rk-graph-children">
      <div v-for="child in node.children" :key="child.id" class="rk-graph-child">
        <AstNodeView
          :node="child"
          :selected-id="props.selectedId"
          :mode="props.mode"
          @select="emit('select', $event)"
          @hover="emit('hover', $event)"
        />
      </div>
    </div>
  </div>

  <div
    v-else-if="mode === 'nested'"
    class="rk-tree-node"
    :class="[`rk-tree-node--${node.kind}`, { 'is-selected': props.selectedId === node.id }]"
    :data-node-id="node.id"
    @click.stop="emit('select', node)"
    @mouseenter="emit('hover', node)"
    @mouseleave="emit('hover')"
  >
    <div class="rk-tree-head">
      <button class="rk-chip" :class="chipClass(node)" type="button">
        {{ title(node) }}
      </button>
      <span class="rk-node-meta">{{ node.kind }}</span>
      <span class="rk-node-span">{{ node.span.startLine }}:{{ node.span.startColumn }}</span>
    </div>
    <div v-if="node.children?.length" class="rk-tree-children">
      <AstNodeView
        v-for="child in node.children"
        :key="child.id"
        :node="child"
        :selected-id="props.selectedId"
        :mode="props.mode"
        @select="emit('select', $event)"
        @hover="emit('hover', $event)"
      />
    </div>
  </div>

  <span
    v-else
    class="rk-chip-wrap"
    :class="{ 'is-selected': props.selectedId === node.id }"
    :data-node-id="node.id"
    @click.stop="emit('select', node)"
    @mouseenter="emit('hover', node)"
    @mouseleave="emit('hover')"
  >
    <button class="rk-chip" :class="chipClass(node)" type="button">{{ title(node) }}</button>
    <template v-if="node.children?.length">
      <span class="rk-chip-pun">(</span>
      <AstNodeView
        v-for="(child, index) in node.children"
        :key="child.id"
        :node="child"
        :selected-id="props.selectedId"
        :mode="props.mode"
        @select="emit('select', $event)"
        @hover="emit('hover', $event)"
      />
      <span class="rk-chip-pun">)</span>
    </template>
  </span>
</template>
