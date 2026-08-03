<template>
  <div>
    <div v-if="loading" class="loading-state"><div class="spinner"></div> 加载中…</div>
    <Card v-else title="缓存与上游快照" subtitle="明确区分空数据和接口不可用" :noPad="true">
      <DataTable :columns="cols" :rows="rows">
        <template #status="{ row }"><StatusChip :status="String(row.status)" /></template>
        <template #updated_at="{ row }">{{ formatDate(String(row.updated_at)) }}</template>
        <template #expires_at="{ row }">{{ row.expires_at ? formatDate(String(row.expires_at)) : '—' }}</template>
        <template #actions="{ row }">
          <div class="row-actions">
            <button class="secondary mini" @click="viewCache(String(row.key))">查看</button>
            <button class="danger mini" @click="deleteCache(String(row.key))">清除</button>
          </div>
        </template>
      </DataTable>
    </Card>

    <Drawer v-if="cacheDetail" :title="`缓存：${cacheDetail.key}`" @close="cacheDetail = null">
      <pre>{{ JSON.stringify(cacheDetail, null, 2) }}</pre>
    </Drawer>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { get, del } from '@/api'
import { notify } from '@/stores/session'
import { formatDate } from '@/utils'
import Card from '@/components/Card.vue'
import DataTable from '@/components/DataTable.vue'
import StatusChip from '@/components/StatusChip.vue'
import Drawer from '@/components/Drawer.vue'

const loading = ref(true)
const rows = ref<any[]>([])
const cacheDetail = ref<any>(null)

const cols = [
  { key: 'key', label: '键', mono: true },
  { key: 'provider', label: 'Provider' },
  { key: 'status', label: '状态' },
  { key: 'record_count', label: '记录数' },
  { key: 'updated_at', label: '更新时间' },
  { key: 'expires_at', label: '过期时间' },
  { key: 'actions', label: '操作' },
]

async function load() {
  loading.value = true
  try { rows.value = await get<any[]>('/cache') }
  catch (e: any) { notify(e.message, 'error') }
  finally { loading.value = false }
}

async function viewCache(key: string) {
  try { cacheDetail.value = await get<any>(`/cache/${encodeURIComponent(key)}`) }
  catch (e: any) { notify(e.message, 'error') }
}

async function deleteCache(key: string) {
  if (!confirm(`清除缓存 "${key}"？`)) return
  try { await del(`/cache/${encodeURIComponent(key)}`); notify('缓存已清除'); await load() }
  catch (e: any) { notify(e.message, 'error') }
}

onMounted(load)
defineExpose({ load })
</script>
