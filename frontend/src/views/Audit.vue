<template>
  <div class="audit-page">
    <div class="alert alert-info mb-4">
      提示：由于安全原因，敏感信息（如密钥、密码等）和较大的请求体不会被记录到审计日志中。
    </div>

    <div v-if="loading" class="loading-state">
      <div class="spinner"></div>
      <p>加载中...</p>
    </div>

    <div class="card" v-else>
      <div class="card-body">
        <div class="table-wrap">
          <table class="table">
            <thead>
              <tr>
                <th>时间</th>
                <th>动作</th>
                <th>目标类型/ID</th>
                <th>结果</th>
                <th>Request ID</th>
                <th>操作</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="log in audits" :key="log.id">
                <td>{{ formatDate(log.createdAt) }}</td>
                <td>{{ log.action }}</td>
                <td>{{ log.targetType }} / {{ log.targetId || '-' }}</td>
                <td><StatusChip :status="log.result" /></td>
                <td class="text-muted">{{ log.requestId }}</td>
                <td>
                  <button class="btn btn-sm btn-secondary" @click="viewAudit(log)">详情</button>
                </td>
              </tr>
              <tr v-if="audits.length === 0">
                <td colspan="6" class="text-center text-muted">暂无审计记录</td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>
    </div>

    <Drawer title="审计详情" ref="auditDrawer">
      <div v-if="currentAudit">
        <div class="kv-grid mb-4">
          <div class="kv-key">操作人:</div>
          <div class="kv-val">{{ currentAudit.operator || '系统' }}</div>
          <div class="kv-key">动作:</div>
          <div class="kv-val">{{ currentAudit.action }}</div>
          <div class="kv-key">目标:</div>
          <div class="kv-val">{{ currentAudit.targetType }} ({{ currentAudit.targetId }})</div>
          <div class="kv-key">结果:</div>
          <div class="kv-val"><StatusChip :status="currentAudit.result" /></div>
          <div class="kv-key">IP:</div>
          <div class="kv-val">{{ currentAudit.ipAddress || '-' }}</div>
        </div>
        <h4>详细信息 (Details)</h4>
        <pre class="log-viewer">{{ JSON.stringify(currentAudit.details, null, 2) }}</pre>
      </div>
    </Drawer>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue';
import { get } from '@/api';
import { notify, openDrawer } from '@/stores/session';
import { formatDate } from '@/utils';
import StatusChip from '@/components/StatusChip.vue';
import Drawer from '@/components/Drawer.vue';

const loading = ref(true);
const audits = ref<any[]>([]);
const currentAudit = ref<any>(null);
const auditDrawer = ref<any>(null);

const load = async () => {
  loading.value = true;
  try {
    const data = await get('/audit?limit=250');
    audits.value = data || [];
  } catch (error: any) {
    notify(error.message || '加载审计记录失败', 'error');
  } finally {
    loading.value = false;
  }
};

const viewAudit = (log: any) => {
  currentAudit.value = log;
  openDrawer('auditDrawer');
};

onMounted(() => {
  load();
  if(auditDrawer.value) auditDrawer.value.id = 'auditDrawer';
});

defineExpose({ load });
</script>
