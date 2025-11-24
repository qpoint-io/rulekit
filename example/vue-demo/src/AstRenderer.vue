<template>
    <div v-if="node" style="font-size: 0.8rem !important; margin-bottom: 0.25rem;">
        <Trio v-if="node.node_type === 'operator'" :rows="trioRows(node)">
            <template #l>
                <AstRenderer v-if="node.left" :node="node.left"
                    :style="!isInline(node.left) ? containerStyle(node.left) : {}" />
            </template>
            <template #o>
                <span style="color: #47c1ff; padding: 0.25rem;">{{ node.operator }}</span>
            </template>
            <template #r>
                <AstRenderer v-if="node.right" :node="node.right"
                    :force-break="node.right.node_type === 'operator' && ['and', 'or'].includes(node.right.operator)"
                    :style="(node.right.node_type === 'operator' && ['and', 'or'].includes(node.right.operator)) ? containerStyle(node.right) : {}" />
            </template>
        </Trio>
        <template v-else-if="node.node_type === 'field'">
            <select :style="{ 'color': 'inherit' }" readonly>
                <option selected>{{ node.name }}</option>
            </select>
        </template>
        <template v-else-if="node.node_type === 'literal'">
            <input :style="{ 'color': literalColor(node.type) }" type="text" :value="node.value" readonly />
        </template>
        <template v-else-if="node.node_type === 'array'">
            <div
                :style="{ 'display': 'inline-flex', 'flex-direction': 'row', 'gap': '0.25rem', 'align-items': 'baseline' }">
                <span style="font-size: 1.25em; margin-right: -0.25rem; font-weight: bold;">[</span>
                <template v-for="(element, index) in node.elements" :key="element">
                    <AstRenderer :node="element" />
                    <span v-if="index < node.elements.length - 1"
                        style="font-weight: bold; font-size: 1.25em; margin-left: -0.25rem;">,</span>
                </template>
                <span style="font-size: 1.25em; margin-left: -0.25rem; font-weight: bold;">]</span>
            </div>
        </template>
        <template v-else-if="node.node_type === 'function'">
            <span>{{ node.name }}</span>
            <div
                :style="{ 'display': 'inline-flex', 'flex-direction': 'row', 'gap': '0.25rem', 'align-items': 'baseline' }">
                <span style="font-size: 1.25em; margin-right: -0.25rem; font-weight: bold;">(</span>
                <template v-for="(element, index) in node.args.elements" :key="element">
                    <AstRenderer :node="element" />
                    <span v-if="index < node.args.elements.length - 1"
                        style="font-weight: bold; font-size: 1.25em; margin-left: -0.25rem;">,</span>
                </template>
                <span style="font-size: 1.25em; margin-left: -0.25rem; font-weight: bold;">)</span>
            </div>
        </template>
        <span v-else>TODO</span>
    </div>
</template>

<style scoped>
input {
    field-sizing: content;
    min-width: 10px;
    max-width: 100px;
    padding: 0.25rem;
}

select {
    field-sizing: content;
    padding: 0.25rem;
    min-width: 10px;
}

* {
    font-family: 'Departure Mono', 'Monaco', 'Menlo', monospace !important;
}
</style>

<script setup lang="ts">
import type { ASTNode, ASTNodeLiteral, ASTNodeOperator } from './ast'
import Trio from './Trio.vue'
type element = 'l' | 'o' | 'r';

const props = defineProps<{
    node: ASTNode,
    forceBreak?: boolean,
}>()

const containerStyle = (node: ASTNode) => {
    if (node.node_type === 'operator') {
        if (['and', 'or'].includes(node.operator) || props.forceBreak) {
            return {
                'background-color': 'rgba(150, 150, 150, 0.1)',
                'padding': '0.25rem',
                'border': '3px solid #1a1a1a',
            }
        }
    }
    return {};
}

const trioRows = (node: ASTNodeOperator): element[][] => {
    switch (node.operator) {
        case 'not':
            return [['o', 'r']];
        case 'in':
        case 'and':
        case 'or':
            if (isInline(node.left) && isInline(node.right) && !props.forceBreak) {
                return [['l', 'o', 'r']];
            } else if (isInline(node.right)) {
                return [['l'], ['o', 'r']];
            } else {
                return [['l'], ['o', 'r']];
            }
        default:
            return [['l', 'o', 'r']];
    }
}

const isInline = (node: ASTNode | null): boolean => {
    if (node === null) {
        return true;
    }

    if (node.node_type === 'array') {
        return !!node.elements.find(element => isInline(element)) === true;
    } else if (node.node_type === 'function') {
        return !!node.args.elements.find(element => isInline(element)) === true;
    } else if (node.node_type === 'field') {
        return true;
    } else if (node.node_type === 'literal') {
        return true;
    } else if (node.node_type === 'operator') {
        if (['in', 'not'].includes(node.operator)) {
            return isInline(node.left) && isInline(node.right);
        }
        return false;
    }
    return true;
}

const literalColor = (type: ASTNodeLiteral['type']): string => {
    switch (type) {
        case 'int64':
            return '#ff9eea';
        case 'float64':
            return '#ff9eea';
        case 'string':
            return '#4ce660';
        case 'bool':
            return '#ff9eea';
        case 'bytes':
            return '#4ce660';
        default:
            console.warn(`Unknown literal type: ${type}`);
            return 'inherit';
    }
};
</script>