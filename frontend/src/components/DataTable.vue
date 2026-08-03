<template>
  <div class="table-wrap">
    <div v-if="loading" class="loading-state"><div class="spinner"></div> 加载中…</div>
    <div v-else-if="!rows || rows.length === 0" class="empty-state">
      <div class="empty-icon">📭</div>
      {{ emptyText || '暂无数据' }}
    </div>
    <table v-else>
      <thead>
        <tr>
          <th v-for="col in columns" :key="col.key">{{ col.label }}</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="(row, i) in rows" :key="i">
          <td v-for="col in columns" :key="col.key" :class="col.mono ? 'mono' : ''">
            <slot :name="col.key" :row="row" :value="row[col.key]">
              {{ row[col.key] ?? '—' }}
            </slot>
          </td>
        </tr>
      </tbody>
    </table>
  </div>
</template>

<script setup lang="ts">
defineProps<{
  columns: { key: string; label: string; mono?: boolean }[]
  rows: Record<string, unknown>[]
  loading?: boolean
  emptyText?: string
}>()
</script>
