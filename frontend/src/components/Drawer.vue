<template>
  <Teleport to="body">
    <div class="overlay" @click.self="$emit('close')">
      <section class="drawer" @click.stop>
        <div class="drawer-header">
          <span class="drawer-title">{{ title }}</span>
          <button class="ghost" style="padding:4px 8px;font-size:18px;line-height:1" @click="$emit('close')">×</button>
        </div>
        <div class="drawer-body">
          <slot />
        </div>
      </section>
    </div>
  </Teleport>
</template>

<script setup lang="ts">
defineProps<{ title: string }>()
defineEmits<{ close: [] }>()
</script>
