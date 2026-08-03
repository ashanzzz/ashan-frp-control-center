<template>
  <div>
    <div v-if="loading" class="loading-state"><div class="spinner"></div> 加载中…</div>
    <template v-else>
      <div class="grid-2">
        <!-- ChmlFrp -->
        <Card title="ChmlFrp" subtitle="OAuth、节点和隧道 API">
          <template #header>
            <StatusChip :status="hasCred('chmlfrp','access_token') ? 'Token 已配置' : '待授权'" />
          </template>
          <form class="form" @submit.prevent="saveForm('chmlfrp')">
            <FormField label="API 地址" name="chmlfrp.base_url" v-model="settings['chmlfrp.base_url']" placeholder="https://cf-v2.uapis.cn" />
            <FormField label="OAuth Token URL" name="chmlfrp.oauth.token_url" v-model="settings['chmlfrp.oauth.token_url']" />
            <FormField label="Device Authorization URL" name="chmlfrp.oauth.device_authorization_url" v-model="settings['chmlfrp.oauth.device_authorization_url']" placeholder="可选" />
            <FormField label="Client ID" name="client_id" type="password" v-model="secrets.client_id" placeholder="留空表示不更新" />
            <FormField label="Client Secret" name="client_secret" type="password" v-model="secrets.client_secret" placeholder="留空表示不更新" />
            <div class="form-actions">
              <button class="secondary" type="button" @click="testChmlFrp">测试 Token</button>
              <button class="primary" type="submit">保存</button>
            </div>
          </form>
        </Card>

        <!-- Cloudflare -->
        <Card title="Cloudflare DNS" subtitle="增量同步受管记录">
          <template #header>
            <StatusChip :status="hasCred('cloudflare','api_token') ? '已配置' : '未配置'" />
          </template>
          <form class="form" @submit.prevent="saveForm('cloudflare')">
            <FormField label="API Token" name="api_token" type="password" v-model="secrets.cf_token" placeholder="留空表示不更新" />
            <FormField label="Zone ID" name="cloudflare.zone_id" v-model="settings['cloudflare.zone_id']" />
            <FormField label="节点 CNAME 模板" name="cloudflare.target_template" v-model="settings['cloudflare.target_template']" placeholder="{node}.ip.chmlfrp.cn" />
            <div class="form-actions">
              <button class="secondary" type="button" @click="testCloudflare">测试并发现 Zone</button>
              <button class="primary" type="submit">保存</button>
            </div>
          </form>
        </Card>

        <!-- Runtime -->
        <Card title="内置 FRPC" subtitle="控制中心容器内直接运行并守护 frpc 进程">
          <template #header><StatusChip status="embedded" /></template>
          <form class="form" @submit.prevent="saveForm('runtime')">
            <FormField label="frpc 二进制" name="runtime.binary_path" v-model="settings['runtime.binary_path']" />
            <FormField label="配置路径" name="runtime.config_path" v-model="settings['runtime.config_path']" />
            <FormField label="日志路径" name="runtime.log_path" v-model="settings['runtime.log_path']" />
            <FormField label="备份目录" name="runtime.backup_dir" v-model="settings['runtime.backup_dir']" />
            <FormField label="容器启动时自动运行 frpc" name="runtime.autostart" type="checkbox" :placeholder="''" v-model="settings['runtime.autostart']" />
            <FormField label="frpc 异常退出后自动重启" name="runtime.auto_restart" type="checkbox" :placeholder="''" v-model="settings['runtime.auto_restart']" />
            <div class="form-actions">
              <button class="primary" type="submit">保存</button>
            </div>
          </form>
        </Card>

        <!-- Unraid -->
        <Card title="Unraid 官方 API（可选）" subtitle="仅用于系统信息和诊断，不再控制 FRPC">
          <template #header>
            <StatusChip :status="hasCred('unraid','api_key') ? '已配置' : '未配置'" />
          </template>
          <form class="form" @submit.prevent="saveForm('unraid')">
            <FormField label="Unraid 地址" name="unraid.base_url" v-model="settings['unraid.base_url']" placeholder="http://192.168.8.11" />
            <FormField label="GraphQL 路径" name="unraid.graphql_path" v-model="settings['unraid.graphql_path']" />
            <FormField label="API Key" name="api_key" type="password" v-model="secrets.unraid_key" placeholder="留空表示不更新" />
            <div class="form-actions">
              <button class="secondary" type="button" @click="testUnraid">测试连接</button>
              <button class="primary" type="submit">保存</button>
            </div>
          </form>
        </Card>

        <!-- Email Webhook -->
        <Card title="验证码邮件 Webhook" subtitle="邮件聚合器提取验证码后推送到本系统">
          <template #header>
            <StatusChip :status="hasCred('email','webhook_token') ? '已配置' : '未配置'" />
          </template>
          <form class="form" @submit.prevent="saveForm('email')">
            <FormField label="启用邮件验证码 Webhook" name="email.webhook_enabled" type="checkbox" :placeholder="''" v-model="settings['email.webhook_enabled']" />
            <FormField label="Webhook Token" name="webhook_token" type="password" v-model="secrets.webhook_token" placeholder="留空表示不更新" />
            <div class="code-block" style="font-size:11px">POST /api/v1/webhooks/email-code<br/>Header: X-Webhook-Token<br/>Body: { challengeId, code }</div>
            <div class="form-actions">
              <button class="primary" type="submit">保存</button>
            </div>
          </form>
        </Card>
      </div>
    </template>

    <Drawer v-if="testResult" :title="testTitle" @close="testResult = null">
      <pre>{{ JSON.stringify(testResult, null, 2) }}</pre>
    </Drawer>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { get, post, put, patch } from '@/api'
import { notify } from '@/stores/session'
import Card from '@/components/Card.vue'
import StatusChip from '@/components/StatusChip.vue'
import FormField from '@/components/FormField.vue'
import Drawer from '@/components/Drawer.vue'

const loading = ref(true)
const settings = ref<Record<string, any>>({})
const credentials = ref<any[]>([])
const secrets = ref<Record<string, string>>({ client_id: '', client_secret: '', cf_token: '', unraid_key: '', webhook_token: '' })
const testResult = ref<any>(null)
const testTitle = ref('')

function hasCred(provider: string, name: string) {
  return credentials.value.some(c => c.provider === provider && c.name === name)
}

async function load() {
  loading.value = true
  try {
    const [s, c] = await Promise.all([get<any>('/settings'), get<any[]>('/credentials')])
    settings.value = s
    credentials.value = c
    secrets.value = { client_id: '', client_secret: '', cf_token: '', unraid_key: '', webhook_token: '' }
  } catch (e: any) { notify(e.message, 'error') }
  finally { loading.value = false }
}

async function saveForm(kind: string) {
  try {
    const toSave: Record<string, any> = {}
    const prefixes: Record<string, string[]> = {
      chmlfrp: ['chmlfrp.base_url', 'chmlfrp.oauth.token_url', 'chmlfrp.oauth.device_authorization_url'],
      cloudflare: ['cloudflare.zone_id', 'cloudflare.target_template'],
      runtime: ['runtime.binary_path', 'runtime.config_path', 'runtime.log_path', 'runtime.backup_dir', 'runtime.autostart', 'runtime.auto_restart'],
      unraid: ['unraid.base_url', 'unraid.graphql_path'],
      email: ['email.webhook_enabled'],
    }
    for (const key of (prefixes[kind] || [])) { if (key in settings.value) toSave[key] = settings.value[key] }
    if (Object.keys(toSave).length) await patch('/settings', toSave)

    const secretMap: Record<string, [string, string] | null> = {
      chmlfrp: null,
      cloudflare: null,
      runtime: null,
      unraid: null,
      email: null,
    }
    const secretPairs: Record<string, [string, string, string][]> = {
      chmlfrp: [['chmlfrp', 'client_id', 'client_id'], ['chmlfrp', 'client_secret', 'client_secret']],
      cloudflare: [['cloudflare', 'api_token', 'cf_token']],
      unraid: [['unraid', 'api_key', 'unraid_key']],
      email: [['email', 'webhook_token', 'webhook_token']],
    }
    for (const [provider, name, key] of (secretPairs[kind] || [])) {
      if (secrets.value[key]) await put(`/credentials/${provider}/${name}`, { secret: secrets.value[key] })
    }
    notify('配置已保存')
    await load()
  } catch (e: any) { notify(e.message, 'error') }
}

async function testChmlFrp() {
  try { testTitle.value = 'ChmlFrp 连接测试'; testResult.value = await post('/providers/chmlfrp/test') } catch (e: any) { notify(e.message, 'error') }
}
async function testCloudflare() {
  try { testTitle.value = 'Cloudflare 连接测试'; testResult.value = await post('/providers/cloudflare/test') } catch (e: any) { notify(e.message, 'error') }
}
async function testUnraid() {
  try { testTitle.value = 'Unraid 连接测试'; testResult.value = await post('/providers/unraid/test') } catch (e: any) { notify(e.message, 'error') }
}

onMounted(load)
defineExpose({ load })
</script>
