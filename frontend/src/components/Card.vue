<template>
  <div class="card">
    <div class="card-header" v-if="$slots.header || title">
      <div>
        <div class="card-title">{{ title }}</div>
        <div class="card-sub" v-if="subtitle">{{ subtitle }}</div>
      </div>
      <div class="btn-group">
        <slot name="header" />
      </div>
    </div>
    <div class="card-body" :style="noPad ? 'padding:0' : ''">
      <slot />
    </div>
  </div>
</template>

<script setup lang="ts">
defineProps<{
  title?: string
  subtitle?: string
  noPad?: boolean
}>()
</script>
