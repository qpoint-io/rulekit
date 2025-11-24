<template>
    <div style="display: inline-flex; flex-direction: row; gap: 0.25rem;">
        <div v-for="row in rows" :key="JSON.stringify(row)"
            style="display:inline-flex; flex-direction: column; gap: 0.25rem;">
            <template v-for="el in row" :key="el">
                <component :is="el" />
            </template>
        </div>
    </div>
</template>

<script setup lang="ts">
type element = 'l' | 'o' | 'r';

defineSlots<{
    l: () => any
    o: () => any
    r: () => any
}>()
defineProps<{
    rows: element[][]
}>()

import { useSlots } from 'vue'


const { l, o, r } = useSlots()
</script>