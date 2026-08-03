<template>
  <span class="chip" :class="cls">{{ label }}</span>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { statusClass } from '@/utils'

const props = defineProps<{ status: string }>()
const cls = computed(() => statusClass(props.status))
const label = computed(() => props.status || '未知')
</script>
