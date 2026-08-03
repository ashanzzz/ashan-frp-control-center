<template>
  <div>
    <div v-if="loading" class="loading-state"><div class="spinner"></div> 加载中…</div>
    <template v-else>
      <!-- ChmlFrp Auth Status -->
      <div class="grid-2">
        <Card title="ChmlFrp OAuth 状态" subtitle="自动刷新、人工 Token 与 Device Code">
          <template #header>
            <StatusChip :status="authStatus?.authenticated ? '已登录' : '需要授权'" />
          </template>
          <div class="kv-grid">
            <span class="kv-key">Access Token</span>
            <span class="kv-val">{{ cred('chmlfrp','access_token') }}</span>
            <span class="kv-key">Refresh Token</span>
            <span class="kv-val">{{ cred('chmlfrp','refresh_token') }}</span>
            <span class="kv-key">状态说明</span>
            <span class="kv-val">{{ authStatus?.error || '有效' }}</span>
          </div>
          <div class="btn-group mt-2">
            <button class="secondary" @click="doAuthEnsure">自动恢复</button>
            <button class="secondary" @click="doAuthRefresh">刷新 Token</button>
            <button class="primary" @click="doDeviceStart">Device 授权</button>
          </div>
        </Card>

        <Card title="人工录入 OAuth" subtitle="从官方登录页面获取后粘贴">
          <form class="form" @submit.prevent="doManualToken">
            <div class="form-field">
              <label class="form-label">Access Token</label>
              <input v-model="manualForm.accessToken" type="password" placeholder="必填" />
            </div>
            <div class="form-field">
              <label class="form-label">Refresh Token</label>
              <input v-model="manualForm.refreshToken" type="password" placeholder="可选" />
            </div>
            <div class="form-field">
              <label class="form-label">过期时间</label>
              <input v-model="manualForm.expiresAt" type="datetime-local" />
            </div>
            <div class="form-actions">
              <button class="primary" type="submit">验证并保存</button>
            </div>
          </form>
        </Card>
      </div>

      <!-- Credential Vault -->
      <Card title="凭据保险库" subtitle="点击显示时需再次输入管理员密码，30 秒后自动隐藏">
        <div style="display:grid;grid-template-columns:repeat(auto-fill,minmax(280px,1fr));gap:10px">
          <div v-for="c in credentials" :key="c.provider+c.name"
            style="display:flex;align-items:center;gap:10px;padding:12px;background:var(--bg-glass);border-radius:var(--radius);border:1px solid var(--border)">
            <div style="flex:1;min-width:0">
              <div style="font-size:11px;color:var(--text-muted)">{{ c.provider }}</div>
              <div style="font-size:13px;font-weight:600;color:var(--text-primary)">{{ c.name }}</div>
              <code style="font-size:11px">{{ c.mask || '—' }}</code>
            </div>
            <StatusChip :status="c.status" />
            <button class="secondary mini" @click="revealCredential(c)">显示</button>
          </div>
          <div v-if="!credentials.length" class="empty-state">暂无凭据</div>
        </div>
      </Card>

      <!-- OAuth Challenges -->
      <Card title="授权会话" subtitle="Device Code 和邮箱验证码会话">
        <DataTable :columns="challengeCols" :rows="challenges">
          <template #status="{ row }"><StatusChip :status="String(row.status)" /></template>
          <template #expires_at="{ row }">{{ formatDate(String(row.expires_at)) }}</template>
          <template #actions="{ row }">
            <div class="row-actions">
              <button v-if="row.kind === 'device' && row.status !== 'completed'" class="secondary mini"
                @click="doDevicePoll(String(row.id))">检查授权</button>
              <button v-if="['pending','waiting_code'].includes(String(row.status))" class="secondary mini"
                @click="openCodeInput(String(row.id))">输入验证码</button>
            </div>
          </template>
        </DataTable>
      </Card>

      <!-- Change Password -->
      <Card title="修改管理员密码" subtitle="修改后全部会话立即失效">
        <form class="form" @submit.prevent="doChangePassword" style="max-width:400px">
          <div class="form-field">
            <label class="form-label">当前密码</label>
            <input v-model="pwForm.currentPassword" type="password" required />
          </div>
          <div class="form-field">
            <label class="form-label">新密码（至少 10 位）</label>
            <input v-model="pwForm.newPassword" type="password" required />
          </div>
          <div class="form-actions">
            <button class="danger" type="submit">修改密码并退出</button>
          </div>
        </form>
      </Card>
    </template>

    <!-- Device Code Drawer -->
    <Drawer v-if="deviceData" title="Device Code 授权" @close="deviceData = null">
      <div style="text-align:center;padding:20px;display:flex;flex-direction:column;gap:16px">
        <div style="font-size:36px;font-weight:800;letter-spacing:6px;color:var(--c-blue);font-family:'JetBrains Mono',monospace">{{ deviceData.userCode }}</div>
        <p style="color:var(--text-muted);font-size:13px">请在官方授权页输入以上代码</p>
        <a class="primary" style="display:inline-flex;align-items:center;justify-content:center;padding:9px 20px;border-radius:var(--radius);text-decoration:none;background:var(--c-blue);color:#fff"
          :href="deviceData.verificationUri" target="_blank" rel="noreferrer">打开授权页面 ↗</a>
        <button class="secondary" @click="doDevicePoll(deviceData.challengeId)">已完成，检查授权</button>
        <small style="color:var(--text-muted)">过期：{{ formatDate(deviceData.expiresAt) }}</small>
      </div>
    </Drawer>

    <!-- Code Input Drawer -->
    <Drawer v-if="codeInputId" title="输入邮箱验证码" @close="codeInputId = null">
      <form class="form" @submit.prevent="submitCode">
        <div class="form-field">
          <label class="form-label">验证码</label>
          <input v-model="codeInput" type="text" placeholder="从邮件中获取" required />
        </div>
        <div class="form-actions">
          <button class="ghost" type="button" @click="codeInputId = null">取消</button>
          <button class="primary" type="submit">提交</button>
        </div>
      </form>
    </Drawer>

    <!-- Reveal Drawer -->
    <Drawer v-if="revealInfo" :title="`${revealInfo.provider}/${revealInfo.name}`" @close="revealInfo = null">
      <div style="padding:20px 0">
        <div class="form-field">
          <label class="form-label">管理员密码</label>
          <input v-model="revealPassword" type="password" placeholder="验证身份后显示" />
        </div>
        <div class="form-actions mt-2">
          <button class="ghost" @click="revealInfo = null">取消</button>
          <button class="primary" @click="confirmReveal">确认显示</button>
        </div>
        <div v-if="revealValue" class="code-block mt-2">{{ revealValue }}</div>
        <p v-if="revealValue" style="font-size:11px;color:var(--text-muted);margin-top:8px">30 秒后自动关闭</p>
      </div>
    </Drawer>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { get, post } from '@/api'
import { notify } from '@/stores/session'
import { formatDate } from '@/utils'
import Card from '@/components/Card.vue'
import StatusChip from '@/components/StatusChip.vue'
import DataTable from '@/components/DataTable.vue'
import Drawer from '@/components/Drawer.vue'

const loading = ref(true)
const authStatus = ref<any>(null)
const credentials = ref<any[]>([])
const challenges = ref<any[]>([])
const manualForm = ref({ accessToken: '', refreshToken: '', expiresAt: '' })
const pwForm = ref({ currentPassword: '', newPassword: '' })
const deviceData = ref<any>(null)
const codeInputId = ref<string | null>(null)
const codeInput = ref('')
const revealInfo = ref<any>(null)
const revealPassword = ref('')
const revealValue = ref('')

const challengeCols = [
  { key: 'kind', label: '类型' },
  { key: 'status', label: '状态' },
  { key: 'session_tag', label: '会话标识' },
  { key: 'expires_at', label: '过期时间' },
  { key: 'actions', label: '操作' },
]

function cred(provider: string, name: string) {
  return credentials.value.find(c => c.provider === provider && c.name === name)?.mask || '未配置'
}

async function load() {
  loading.value = true
  try {
    const [s, c, ch] = await Promise.all([
      get<any>('/providers/chmlfrp/auth-status'),
      get<any[]>('/credentials'),
      get<any[]>('/oauth/challenges'),
    ])
    authStatus.value = s
    credentials.value = c
    challenges.value = ch
  } catch (e: any) { notify(e.message, 'error') }
  finally { loading.value = false }
}

async function doAuthEnsure() {
  try { await post('/providers/chmlfrp/auth/ensure'); notify('认证恢复任务已入队') } catch (e: any) { notify(e.message, 'error') }
}
async function doAuthRefresh() {
  try { await post('/providers/chmlfrp/auth/refresh'); notify('Token 刷新任务已入队') } catch (e: any) { notify(e.message, 'error') }
}
async function doDeviceStart() {
  try { deviceData.value = await post<any>('/providers/chmlfrp/auth/device/start') } catch (e: any) { notify(e.message, 'error') }
}
async function doDevicePoll(id: string) {
  try {
    const result = await post<any>('/providers/chmlfrp/auth/device/poll', { challengeId: id })
    notify('Device Code 授权成功')
    deviceData.value = null
    await load()
  } catch (e: any) { notify(e.message, 'error') }
}
async function doManualToken() {
  try {
    await post('/providers/chmlfrp/auth/manual', manualForm.value)
    notify('Token 已保存')
    manualForm.value = { accessToken: '', refreshToken: '', expiresAt: '' }
    await load()
  } catch (e: any) { notify(e.message, 'error') }
}
async function doChangePassword() {
  try {
    await post('/auth/change-password', pwForm.value)
    notify('密码已修改，请重新登录')
    setTimeout(() => location.reload(), 1500)
  } catch (e: any) { notify(e.message, 'error') }
}
function openCodeInput(id: string) { codeInputId.value = id; codeInput.value = '' }
async function submitCode() {
  try {
    await post(`/oauth/challenges/${codeInputId.value}/code`, { code: codeInput.value })
    notify('验证码已提交')
    codeInputId.value = null
    await load()
  } catch (e: any) { notify(e.message, 'error') }
}
function revealCredential(c: any) { revealInfo.value = c; revealPassword.value = ''; revealValue.value = '' }
async function confirmReveal() {
  try {
    const data = await post<any>(`/credentials/${revealInfo.value.provider}/${revealInfo.value.name}/reveal`, { password: revealPassword.value })
    revealValue.value = data.value
    setTimeout(() => { revealInfo.value = null; revealValue.value = '' }, 30000)
  } catch (e: any) { notify(e.message, 'error') }
}

onMounted(load)
defineExpose({ load })
</script>
