<script setup lang="ts">
// AST Node types
export interface AstNode {
  node_type: 'operator' | 'field' | 'literal' | 'array'
  operator?: string
  name?: string
  type?: string
  value?: any
  left?: AstNode | null
  right?: AstNode | null
  elements?: AstNode[]
}

const props = defineProps<{
  node: AstNode
  isLast?: boolean
  parentOperator?: string
}>()

// Operator display names
const operatorDisplayNames: Record<string, string> = {
  'eq': 'equals',
  'ne': 'not equals',
  'gt': 'greater than',
  'gte': 'greater than or equal',
  'lt': 'less than',
  'lte': 'less than or equal',
  'contains': 'contains',
  'matches': 'matches',
  'in': 'in',
  'starts_with': 'starts with',
  'ends_with': 'ends with',
  'wildcard': 'wildcard',
  'and': 'And',
  'or': 'Or',
  'not': 'Not',
}

// Helper to format value display
function formatValue(node: AstNode): string {
  if (node.node_type === 'literal') {
    if (node.type === 'regex') {
      return node.value || ''
    }
    if (node.type === 'string') {
      return `"${node.value}"`
    }
    return String(node.value)
  }
  if (node.node_type === 'array') {
    return `[${node.elements?.map(e => formatValue(e)).join(', ') || ''}]`
  }
  if (node.node_type === 'field') {
    return node.name || ''
  }
  return ''
}

// Helper to get field name from node
function getFieldName(node: AstNode | null | undefined): string {
  if (!node) return ''
  if (node.node_type === 'field') {
    return node.name || ''
  }
  if (node.node_type === 'operator' && node.left) {
    return getFieldName(node.left)
  }
  return ''
}

// Helper to check if operator is logical (and/or)
function isLogicalOperator(op: string | undefined): boolean {
  return op === 'and' || op === 'or'
}

// Helper to check if operator is comparison
function isComparisonOperator(op: string | undefined): boolean {
  if (!op) return false
  return !isLogicalOperator(op) && op !== 'not'
}
</script>

<template>
  <div v-if="node">
    <!-- Logical operators (and/or) - render children vertically with connectors -->
    <div v-if="node.node_type === 'operator' && isLogicalOperator(node.operator)" class="ast-logical-group">
      <div v-if="node.left" class="ast-logical-item">
        <AstRenderer :node="node.left" :parent-operator="node.operator" />
        <div v-if="node.right" class="ast-connector">
          <div class="ast-connector-line"></div>
          <button class="ast-connector-button">{{ node.operator === 'and' ? 'And' : 'Or' }}</button>
          <div class="ast-connector-line"></div>
        </div>
      </div>
      <div v-if="node.right" class="ast-logical-item">
        <AstRenderer :node="node.right" :is-last="true" :parent-operator="node.operator" />
      </div>
    </div>

    <!-- NOT operator -->
    <div v-else-if="node.node_type === 'operator' && node.operator === 'not'" class="ast-rule-block">
      <div class="ast-rule-row">
        <div class="ast-field">Not</div>
        <div class="ast-operator"></div>
        <div class="ast-value">
          <AstRenderer v-if="node.right" :node="node.right" />
        </div>
      </div>
      <div class="ast-actions">
        <button class="ast-remove-btn">×</button>
      </div>
    </div>

    <!-- Comparison operators - render as rule block -->
    <div v-else-if="node.node_type === 'operator' && isComparisonOperator(node.operator)" class="ast-rule-block">
      <div class="ast-rule-row">
        <div class="ast-field">{{ getFieldName(node.left) || 'Select...' }}</div>
        <div class="ast-operator">{{ operatorDisplayNames[node.operator || ''] || node.operator || 'Select...' }}</div>
        <div class="ast-value">
          <template v-if="node.right">
            <template v-if="node.right.node_type === 'array'">
              {{ formatValue(node.right) }}
              <div class="ast-value-hint">
                e.g. {{ node.right.elements?.slice(0, 2).map(e => formatValue(e)).join(', ') }}{{ (node.right.elements?.length || 0) > 2 ? '...' : '' }}
              </div>
            </template>
            <template v-else>
              {{ formatValue(node.right) }}
            </template>
          </template>
        </div>
      </div>
      <div class="ast-actions">
        <button class="ast-logic-btn">And</button>
        <button class="ast-logic-btn">Or</button>
        <button class="ast-remove-btn">×</button>
      </div>
    </div>

    <!-- Field nodes -->
    <div v-else-if="node.node_type === 'field'" class="ast-field-display">
      {{ node.name || '' }}
    </div>

    <!-- Literal nodes -->
    <div v-else-if="node.node_type === 'literal'" class="ast-literal-display">
      {{ formatValue(node) }}
    </div>

    <!-- Array nodes -->
    <div v-else-if="node.node_type === 'array'" class="ast-array-display">
      {{ formatValue(node) }}
    </div>

    <!-- Fallback - render children if they exist -->
    <div v-else-if="node.left || node.right" class="ast-group">
      <AstRenderer v-if="node.left" :node="node.left" />
      <AstRenderer v-if="node.right" :node="node.right" />
    </div>
  </div>
</template>

