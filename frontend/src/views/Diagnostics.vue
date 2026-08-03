<template>
  <div>
    <div class="grid-2">
      <!-- Quick Diagnostics -->
      <Card title="上游 API 快速诊断" subtitle="结果仅在当前浏览器抽屉中显示，不写入存储">
        <div style="display:flex;flex-wrap:wrap;gap:8px">
          <button class="secondary" @click="diag('unraid-containers')">Unraid 容器</button>
          <button class="secondary" @click="diag('unraid-mutations')">Unraid Mutation</button>
          <button class="secondary" @click="diag('chmlfrp-nodes')">ChmlFrp 原始节点</button>
          <button class="secondary" @click="diag('chmlfrp-tunnels')">ChmlFrp 原始隧道</button>
          <button class="secondary" @click="diag('cloudflare-zones')">Cloudflare Zones</button>
        </div>
      </Card>

      <!-- GraphQL Debug -->
      <Card title="Unraid GraphQL 调试" subtitle="仅管理员可用；请求通过后端注入 API Key">
        <form class="form" @submit.prevent="runGraphQL">
          <div class="form-field">
            <label class="form-label">GraphQL Query</label>
            <textarea v-model="gqlQuery" rows="8" />
          </div>
          <div class="form-field">
            <label class="form-label">Variables（JSON）</label>
            <textarea v-model="gqlVars" rows="3" />
          </div>
          <div class="form-actions">
            <button class="primary" type="submit">执行查询</button>
          </div>
        </form>
      </Card>
    </div>

    <Card title="安全边界" subtitle="调试结果会经过统一错误处理">
      <ul style="display:flex;flex-direction:column;gap:6px;padding-left:20px;color:var(--text-muted);font-size:13px">
        <li>凭据只由后端读取和注入，不会回显到前端响应。</li>
        <li>完整 Token 只能在认证中心重新验证管理员密码后查看。</li>
        <li>原始响应可从本页或缓存中心查看。</li>
      </ul>
    </Card>

    <Drawer v-if="diagResult" :title="diagTitle" @close="diagResult = null">
      <pre>{{ JSON.stringify(diagResult, null, 2) }}</pre>
    </Drawer>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import { get, post } from '@/api'
import { notify } from '@/stores/session'
import Card from '@/components/Card.vue'
import Drawer from '@/components/Drawer.vue'

const diagResult = ref<any>(null)
const diagTitle = ref('')
const gqlQuery = ref('query Debug {\n  info { os { platform distro release uptime } }\n  dockerContainers { id names state status image }\n}')
const gqlVars = ref('{}')

const diagActions: Record<string, [string, string]> = {
  'unraid-containers': ['/providers/unraid/containers', 'Unraid 容器原始结果'],
  'unraid-mutations': ['/providers/unraid/mutations', 'Unraid Mutation Schema'],
  'chmlfrp-nodes': ['/providers/chmlfrp/raw/nodes', 'ChmlFrp 原始节点'],
  'chmlfrp-tunnels': ['/providers/chmlfrp/raw/tunnels', 'ChmlFrp 原始隧道'],
  'cloudflare-zones': ['/providers/cloudflare/zones', 'Cloudflare Zones'],
}

async function diag(key: string) {
  const [path, title] = diagActions[key]
  try { diagTitle.value = title; diagResult.value = await get(path) }
  catch (e: any) { notify(e.message, 'error') }
}

async function runGraphQL() {
  try {
    let variables = {}
    try { variables = JSON.parse(gqlVars.value || '{}') } catch { throw new Error('Variables 不是有效 JSON') }
    diagTitle.value = 'GraphQL 执行结果'
    diagResult.value = await post('/providers/unraid/graphql', { query: gqlQuery.value, variables })
  } catch (e: any) { notify(e.message, 'error') }
}

function load() {}
defineExpose({ load })
</script>
