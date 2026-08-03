<template>
  <div>
    <div v-if="loading" class="loading-state"><div class="spinner"></div> 加载中…</div>
    <template v-else>
      <div class="grid-2">
        <Card title="自动故障切换" subtitle="连续失败、冷却、Ban 与候选节点约束">
          <template #header><StatusChip :status="settings['automation.enabled'] ? 'enabled' : 'disabled'" /></template>
          <form class="form" @submit.prevent="saveAutomation">
            <FormField label="启用自动故障切换" name="automation.enabled" type="checkbox" :placeholder="''" v-model="settings['automation.enabled']" />
            <FormField label="健康检查间隔（秒）" name="automation.health_interval_seconds" type="number" v-model="settings['automation.health_interval_seconds']" />
            <FormField label="连续失败阈值" name="automation.failure_threshold" type="number" v-model="settings['automation.failure_threshold']" />
            <FormField label="切换后连续恢复确认次数" name="automation.recovery_threshold" type="number" v-model="settings['automation.recovery_threshold']" />
            <FormField label="切换冷却（分钟）" name="automation.cooldown_minutes" type="number" v-model="settings['automation.cooldown_minutes']" />
            <FormField label="失败节点 Ban（分钟）" name="automation.ban_minutes" type="number" v-model="settings['automation.ban_minutes']" />
            <FormField label="最大延迟（ms）" name="automation.max_latency_ms" type="number" v-model="settings['automation.max_latency_ms']" />
            <FormField label="最大丢包（%）" name="automation.max_packet_loss" type="number" v-model="settings['automation.max_packet_loss']" />
            <FormField label="高风险自动切换需人工确认" name="automation.require_approval_for_high_risk" type="checkbox" :placeholder="''" v-model="settings['automation.require_approval_for_high_risk']" />
            <div class="form-actions">
              <button class="primary" type="submit">保存策略</button>
            </div>
          </form>
        </Card>

        <Card title="当前健康依据" subtitle="自动化只在多层检测连续失败后触发">
          <template #header><StatusChip :status="health.overallStatus || 'unknown'" /></template>
          <div class="layer-list">
            <div v-for="layer in health.layers" :key="layer.key" class="layer-item">
              <StatusChip :status="layer.status" />
              <div class="layer-info">
                <div class="layer-name">{{ layer.key }}</div>
                <div class="layer-msg">{{ layer.message }}</div>
              </div>
            </div>
            <div v-if="!health.layers?.length" class="empty-state">暂无健康快照</div>
          </div>
          <div class="btn-group mt-2">
            <button class="secondary" @click="runHealth">立即执行一次检测</button>
          </div>
        </Card>
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { get, post, patch } from '@/api'
import { notify } from '@/stores/session'
import Card from '@/components/Card.vue'
import StatusChip from '@/components/StatusChip.vue'
import FormField from '@/components/FormField.vue'

const loading = ref(true)
const settings = ref<Record<string, any>>({})
const health = ref<any>({})

async function load() {
  loading.value = true
  try {
    const [s, h] = await Promise.all([get<any>('/settings'), get<any>('/system/health')])
    settings.value = s
    health.value = h
  } catch (e: any) { notify(e.message, 'error') }
  finally { loading.value = false }
}

async function saveAutomation() {
  try {
    const keys = ['automation.enabled','automation.health_interval_seconds','automation.failure_threshold','automation.recovery_threshold','automation.cooldown_minutes','automation.ban_minutes','automation.max_latency_ms','automation.max_packet_loss','automation.require_approval_for_high_risk']
    const toSave: Record<string, any> = {}
    for (const k of keys) toSave[k] = settings.value[k]
    await patch('/settings', toSave)
    notify('策略已保存')
  } catch (e: any) { notify(e.message, 'error') }
}

async function runHealth() {
  try { await post('/system/health/run'); notify('健康检查已入队') }
  catch (e: any) { notify(e.message, 'error') }
}

onMounted(load)
defineExpose({ load })
</script>
