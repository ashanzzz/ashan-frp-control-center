<template>
  <div class="dashboard">
    <div class="card mb-4" v-if="!loading && data">
      <div class="card-header">
        <h2 class="card-title">系统概览</h2>
        <div class="toolbar-actions">
          <button class="btn btn-primary" @click="runHealthCheck" :disabled="actionLoading">
            {{ actionLoading ? '检测中...' : '立即检测' }}
          </button>
        </div>
      </div>
      <div class="card-body">
        <div class="hero-panel mb-4">
          <div class="hero-item">
            <span class="hero-label">系统状态: </span>
            <StatusChip :status="data.health.status" />
          </div>
          <div class="hero-item">
            <span class="hero-label">当前节点: </span>
            <span class="hero-value">{{ data.currentNode || '无' }}</span>
          </div>
          <div class="hero-item">
            <span class="hero-label">FRPC状态: </span>
            <StatusChip :status="data.runtime.status" />
          </div>
        </div>
        
        <div class="grid-3 mb-4">
          <div class="metric-card">
            <div class="metric-label">在线节点</div>
            <div class="metric-value">{{ data.counts.onlineNodes }} / {{ data.counts.totalNodes }}</div>
          </div>
          <div class="metric-card">
            <div class="metric-label">期望隧道</div>
            <div class="metric-value">{{ data.counts.desiredTunnels }}</div>
          </div>
          <div class="metric-card">
            <div class="metric-label">期望DNS</div>
            <div class="metric-value">{{ data.counts.desiredDns }}</div>
          </div>
          <div class="metric-card">
            <div class="metric-label">活动任务</div>
            <div class="metric-value">{{ data.counts.activeJobs }}</div>
          </div>
          <div class="metric-card">
            <div class="metric-label">自动切换</div>
            <div class="metric-value">{{ data.automation.enabled ? '已开启' : '已关闭' }}</div>
          </div>
          <div class="metric-card">
            <div class="metric-label">内置FRPC</div>
            <div class="metric-value">{{ data.runtime.version }}</div>
          </div>
        </div>

        <div class="grid-2">
          <div class="health-layers">
            <h3>健康层级</h3>
            <div class="layer-list mt-2">
              <div class="layer-item" v-for="layer in data.health.layers" :key="layer.name">
                <div class="layer-name">{{ layer.name }}</div>
                <StatusChip :status="layer.status" />
                <div class="layer-msg text-muted">{{ layer.message }}</div>
              </div>
            </div>
          </div>
          
          <div class="recent-jobs">
            <h3>最近任务</h3>
            <DataTable :columns="jobColumns" :data="data.recentJobs" />
          </div>
        </div>
      </div>
    </div>

    <div class="card" v-if="!loading && data">
      <div class="card-header">
        <h3 class="card-title">最近切换记录</h3>
      </div>
      <div class="card-body">
        <DataTable :columns="switchColumns" :data="data.recentSwitches" />
      </div>
    </div>

    <div v-if="loading" class="loading-state">
      <div class="spinner"></div>
      <p>加载中...</p>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue';
import { get, post } from '@/api';
import { notify } from '@/stores/session';
import { formatDate } from '@/utils';
import StatusChip from '@/components/StatusChip.vue';
import DataTable from '@/components/DataTable.vue';

const loading = ref(true);
const actionLoading = ref(false);
const data = ref<any>(null);

const jobColumns = [
  { key: 'type', label: '类型' },
  { key: 'status', label: '状态', render: (val: string) => `<StatusChip status="${val}" />` },
  { key: 'createdAt', label: '时间', render: (val: string) => formatDate(val) }
];

const switchColumns = [
  { key: 'createdAt', label: '时间', render: (val: string) => formatDate(val) },
  { key: 'sourceNode', label: '源节点' },
  { key: 'targetNode', label: '目标节点' },
  { key: 'risk', label: '风险' },
  { key: 'status', label: '状态', render: (val: string) => `<StatusChip status="${val}" />` },
  { key: 'actions', label: '操作' } // Slot could be used here if needed
];

const load = async () => {
  loading.value = true;
  try {
    data.value = await get('/system/dashboard');
  } catch (error: any) {
    notify(error.message || '加载仪表盘失败', 'error');
  } finally {
    loading.value = false;
  }
};

const runHealthCheck = async () => {
  actionLoading.value = true;
  try {
    await post('/system/health/run');
    notify('健康检测已触发', 'success');
    await load();
  } catch (error: any) {
    notify(error.message || '触发健康检测失败', 'error');
  } finally {
    actionLoading.value = false;
  }
};

onMounted(() => {
  load();
});

defineExpose({ load });
</script>
