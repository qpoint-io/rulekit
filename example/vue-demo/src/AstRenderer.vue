<template>
    <div style="font-size: 0.8rem !important;" v-if="node" :style="opStyle2(node).container">
        <template v-if="node.node_type === 'operator'" :class="{ 'alternate': alternate }">
            <!-- style="display: flex; align-items: center;" :style="{
                'flex-direction': node.operator == 'not' ? 'row' : 'column',
            }" -->
            <!--template v-if="node.operator == 'not'">
                <span style="color: #47c1ff;">not</span>
                <div :style="{
                    'display': hasChildren(node.right) ? 'inline' : 'inline',
                }">
                    <AstRenderer v-if="node.right" :node="node.right" :alternate="!alternate" />
                </div>
            </template>
<template v-else -->
            <AstRenderer v-if="node.left" :node="node.left" :alternate="!alternate" :style="opStyle2(node).left" />
            <span style="color: #47c1ff;" :style="opStyle2(node).operator">{{ node.operator }}</span>
            <AstRenderer v-if="node.right" :node="node.right" :alternate="!alternate" :style="opStyle2(node).right" />
        </template>
        <template v-else-if="node.node_type === 'field'">
            <select :style="{ 'color': 'inherit' }" readonly>
                <option selected>{{ node.name }}</option>
            </select>
        </template>
        <template v-else-if="node.node_type === 'literal'">
            <input :style="{ 'color': literalColor(node.type) }" type="text" :value="node.value" />
        </template>
        <template v-else-if="node.node_type === 'array'">
            <div
                :style="{ 'display': 'inline-flex', 'flex-direction': 'row', 'gap': '0.25rem', 'align-items': 'center' }">
                <span style="font-size: 1.25em; margin-right: -0.25rem; font-weight: bold;">[</span>
                <template v-for="(element, index) in node.elements" :key="element">
                    <AstRenderer :node="element" :alternate="!alternate" />
                    <span v-if="index < node.elements.length - 1"
                        style="font-weight: bold; font-size: 1.25em; margin-left: -0.25rem;">,</span>
                </template>
                <span style="font-size: 1.25em; margin-left: -0.25rem; font-weight: bold;">]</span>
            </div>
        </template>
        <template v-else-if="node.node_type === 'function'">
            <span>{{ node.name }}</span>
            <div
                :style="{ 'display': 'inline-flex', 'flex-direction': 'row', 'gap': '0.25rem', 'align-items': 'center' }">
                <span style="font-size: 1.25em; margin-right: -0.25rem; font-weight: bold;">(</span>
                <template v-for="(element, index) in node.args.elements" :key="element">
                    <AstRenderer :node="element" :alternate="!alternate" />
                    <span v-if="index < node.args.elements.length - 1"
                        style="font-weight: bold; font-size: 1.25em; margin-left: -0.25rem;">,</span>
                </template>
                <span style="font-size: 1.25em; margin-left: -0.25rem; font-weight: bold;">)</span>
            </div>
        </template>
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
</style>

<script setup lang="ts">
import type { ASTNode, ASTNodeLiteral, ASTNodeOperator } from './ast'

defineProps<{
    node: ASTNode,
    alternate?: boolean
}>()

const opStyle = (node: ASTNode) => {
    return {
        'display': 'grid',
        'grid-template-columns': 'repeat(3, fit-content(1000px))',
        'grid-template-rows': 'repeat(3, fit-content(1000px))',
        'align-items': 'center',
        'gap': '0.25rem',
    }
}

const opStyle2 = (node: ASTNode) => {
    let
        container = opStyle(node),
        left = {
            'grid-area': '1 / 1 / 2 / 2',
        },
        operator = {
            'grid-area': '1 / 2 / 2 / 3',
        },
        right = {
            'grid-area': '1 / 3 / 2 / 4',
        };
    if (node.node_type === 'operator') {
        switch (node.operator) {
            case 'and':
            case 'or':
                if (!hasChildren(node.left) && !hasChildren(node.right)) {
                    container['grid-template-columns'] = 'repeat(3, fit-content(1000px))';
                    container['grid-template-rows'] = 'fit-content(1000px)';
                    left = {
                        'grid-area': '1 / 1 / 2 / 2',
                    }
                    operator = {
                        'grid-area': '1 / 2 / 2 / 3',
                    }
                    right = {
                        'grid-area': '1 / 3 / 2 / 4',
                    }
                } else {
                    left = {
                        'grid-area': '1 / 1 / 2 / 2',
                    }
                    operator = {
                        'grid-area': '2 / 1 / 3 / 2',
                    }
                    if (hasChildren(node.right)) {
                        right = {
                            'grid-area': '2 / 2 / 3 / 3',
                        }
                    } else {
                        right = {
                            'grid-area': '2 / 2 / 3 / 3',
                        }
                    }
                }
                break;
            case 'not':
                left = {
                    'grid-area': '1 / 2 / 2 / 3',
                }
                operator = {
                    'grid-area': '1 / 1 / 2 / 2',
                }
                break;
            case 'in':
                if (hasChildren(node.left)) {

                } else {

                }
                break;
            default:
                left = {
                    'grid-area': '1 / 1 / 2 / 2',
                }
                operator = {
                    'grid-area': '1 / 2 / 2 / 3',
                }
                right = {
                    'grid-area': '1 / 3 / 2 / 4',
                }
                break;
        }
    }

    return {
        container, left, operator, right,
    }
}


const hasChildren = (node: ASTNode | null): boolean => {
    if (node === null) {
        return false;
    }

    switch (node.node_type) {
        case 'operator':
            if (node.right?.node_type === 'operator' || node.left?.node_type === 'operator') {
                return true;
            }
            if (node.operator == "not") {
                return hasChildren(node.right);
            } else {
                return hasChildren(node.left) || hasChildren(node.right);
            }
        case 'array':
            return node.elements.find(element => hasChildren(element)) !== undefined;
        case 'function':
            return node.args.elements.find(element => hasChildren(element)) !== undefined;
        case 'field':
            return false;
        case 'literal':
            return false;
        default:
            return false;
    }
};

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