<template>
  <div class="form-field">
    <label class="form-label" :for="name">{{ label }}</label>
    <textarea v-if="type === 'textarea'" :id="name" :name="name" :placeholder="placeholder"
      :rows="rows || 6" :value="modelValue" @input="$emit('update:modelValue', ($event.target as HTMLTextAreaElement).value)" />
    <select v-else-if="type === 'select'" :id="name" :name="name"
      :value="modelValue" @change="$emit('update:modelValue', ($event.target as HTMLSelectElement).value)">
      <option v-for="opt in options" :key="String(opt.value)" :value="opt.value">{{ opt.label }}</option>
    </select>
    <div v-else-if="type === 'checkbox'" class="toggle-field">
      <input type="checkbox" :id="name" :name="name" :checked="!!modelValue"
        @change="$emit('update:modelValue', ($event.target as HTMLInputElement).checked)" />
      <label class="toggle-label" :for="name">{{ placeholder }}</label>
    </div>
    <input v-else :id="name" :name="name" :type="type || 'text'" :placeholder="placeholder"
      :value="modelValue" @input="$emit('update:modelValue', ($event.target as HTMLInputElement).value)" />
    <div class="form-hint" v-if="hint">{{ hint }}</div>
  </div>
</template>

<script setup lang="ts">
defineProps<{
  label: string
  name: string
  modelValue?: unknown
  type?: string
  placeholder?: string
  hint?: string
  rows?: number
  options?: { value: unknown; label: string }[]
}>()
defineEmits<{ 'update:modelValue': [value: unknown] }>()
</script>
