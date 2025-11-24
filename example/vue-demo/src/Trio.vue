<template>
    <div style="display: inline-block;">
        <div v-for="row in rows" :key="JSON.stringify(row)" style="display:flex; flex-direction: row; gap: 0.25rem;"
            :style="{
                'align-items': row.length == 3 ? 'baseline' : 'start',
            }">
            <template v-for="el in row" :key="el">
                <slot :name="el" />
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
</script>