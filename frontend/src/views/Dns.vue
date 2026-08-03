<template>
  <div>
    <div v-if="loading" class="loading-state"><div class="spinner"></div> 加载中…</div>
    <template v-else>
      <div class="toolbar">
        <div class="toolbar-info">
          <strong>{{ desired.length }}</strong> 条期望 DNS ·
          <strong>{{ observed.length }}</strong> 条远端记录
        </div>
        <div class="toolbar-actions">
          <button class="secondary" @click="dnsDerive">从隧道派生</button>
          <button class="secondary" @click="dnsSync">同步 Cloudflare</button>
          <button class="secondary" @click="dnsPlan">差异预览</button>
          <button class="primary" @click="dnsApply">应用差异</button>
          <button class="primary" @click="openAdd">新增记录</button>
        </div>
      </div>

      <div class="grid-2">
        <Card title="期望 DNS" subtitle="只修改系统创建或认领的记录" :noPad="true">
          <DataTable :columns="desiredCols" :rows="desired">
            <template #name="{ row }"><span class="font-mono">{{ row.name }}</span></template>
            <template #content="{ row }"><span class="font-mono">{{ row.content }}</span></template>
            <template #proxied="{ row }">{{ row.proxied ? '开启' : '关闭' }}</template>
            <template #enabled="{ row }"><StatusChip :status="row.enabled ? 'enabled' : 'disabled'" /></template>
            <template #actions="{ row }">
              <div class="row-actions">
                <button class="secondary mini" @click="openEdit(row)">编辑</button>
                <button class="danger mini" @click="deleteDns(String(row.id))">删除</button>
              </div>
            </template>
          </DataTable>
        </Card>

        <Card title="Cloudflare 观测" subtitle="原生记录默认只读，可手动认领" :noPad="true">
          <DataTable :columns="observedCols" :rows="observed">
            <template #name="{ row }"><span class="font-mono">{{ row.name }}</span></template>
            <template #content="{ row }"><span class="font-mono">{{ row.content }}</span></template>
            <template #proxied="{ row }">{{ row.proxied ? '开启' : '关闭' }}</template>
            <template #managed="{ row }">
              <button class="secondary mini" @click="claimDns(String(row.external_id), !row.managed)">
                {{ row.managed ? '取消认领' : '认领' }}
              </button>
            </template>
          </DataTable>
        </Card>
      </div>
    </template>

    <!-- Add/Edit Drawer -->
    <Drawer v-if="editItem !== null" :title="editItem.id ? '编辑 DNS 记录' : '新增 DNS 记录'" @close="editItem = null">
      <form class="form" @submit.prevent="saveDns">
        <FormField label="记录名称" name="name" v-model="editItem.name" placeholder="app.example.com" />
        <FormField label="类型" name="type" type="select" v-model="editItem.type"
          :options="['CNAME','A','AAAA','TXT','MX','CAA'].map(t => ({ value: t, label: t }))" />
        <FormField label="内容" name="content" v-model="editItem.content" />
        <FormField label="TTL" name="ttl" type="number" v-model="editItem.ttl" />
        <FormField label="Cloudflare 代理" name="proxied" type="checkbox" :placeholder="''" v-model="editItem.proxied" />
        <FormField label="启用" name="enabled" type="checkbox" :placeholder="''" v-model="editItem.enabled" />
        <div class="form-actions">
          <button class="ghost" type="button" @click="editItem = null">取消</button>
          <button class="primary" type="submit">保存</button>
        </div>
      </form>
    </Drawer>

    <!-- Plan / Result Drawer -->
    <Drawer v-if="planResult" :title="planTitle" @close="planResult = null">
      <pre>{{ JSON.stringify(planResult, null, 2) }}</pre>
    </Drawer>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { get, post, put, del } from '@/api'
import { notify } from '@/stores/session'
import Card from '@/components/Card.vue'
import DataTable from '@/components/DataTable.vue'
import StatusChip from '@/components/StatusChip.vue'
import Drawer from '@/components/Drawer.vue'
import FormField from '@/components/FormField.vue'

const loading = ref(true)
const desired = ref<any[]>([])
const observed = ref<any[]>([])
const editItem = ref<any>(null)
const planResult = ref<any>(null)
const planTitle = ref('')

const desiredCols = [
  { key: 'name', label: '名称' }, { key: 'type', label: '类型' },
  { key: 'content', label: '内容' }, { key: 'proxied', label: '代理' },
  { key: 'enabled', label: '状态' }, { key: 'actions', label: '操作' },
]
const observedCols = [
  { key: 'name', label: '名称' }, { key: 'type', label: '类型' },
  { key: 'content', label: '内容' }, { key: 'proxied', label: '代理' },
  { key: 'managed', label: '权限' },
]

async function load() {
  loading.value = true
  try {
    const data = await get<any>('/dns')
    desired.value = data.desired || []
    observed.value = data.observed || []
  } catch (e: any) { notify(e.message, 'error') }
  finally { loading.value = false }
}

function openAdd() { editItem.value = { name: '', type: 'CNAME', content: '', ttl: 1, proxied: false, enabled: true } }
function openEdit(row: any) { editItem.value = { ...row } }

async function saveDns() {
  const item = editItem.value
  try {
    item.ttl = Number(item.ttl) || 1
    if (item.id) await put(`/dns/${item.id}`, item)
    else await post('/dns', item)
    editItem.value = null
    notify('DNS 记录已保存')
    await load()
  } catch (e: any) { notify(e.message, 'error') }
}

async function deleteDns(id: string) {
  if (!confirm('删除本地期望 DNS 定义？远端记录不会立即删除。')) return
  try { await del(`/dns/${id}`); notify('已删除'); await load() }
  catch (e: any) { notify(e.message, 'error') }
}

async function dnsDerive() {
  try { await post('/dns/derive', {}); notify('已从 HTTP/HTTPS 隧道派生 DNS 期望'); await load() }
  catch (e: any) { notify(e.message, 'error') }
}
async function dnsSync() {
  try { await post('/dns/sync'); notify('Cloudflare 同步已入队') }
  catch (e: any) { notify(e.message, 'error') }
}
async function dnsPlan() {
  try { planTitle.value = 'DNS 差异计划'; planResult.value = await post('/dns/plan', {}) }
  catch (e: any) { notify(e.message, 'error') }
}
async function dnsApply() {
  if (!confirm('确认应用受管 DNS 差异？未认领记录不会被覆盖。')) return
  try { await post('/dns/apply', {}); notify('DNS 对账任务已入队', 'warning') }
  catch (e: any) { notify(e.message, 'error') }
}
async function claimDns(externalId: string, managed: boolean) {
  try { await post(`/dns/${externalId}/claim`, { managed }); await load() }
  catch (e: any) { notify(e.message, 'error') }
}

onMounted(load)
defineExpose({ load })
</script>
