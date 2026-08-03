<template>
  <div class="runtime-page">
    <div v-if="loading" class="loading-state">
      <div class="spinner"></div>
      <p>加载中...</p>
    </div>
    
    <div v-else-if="data">
      <div class="grid-3 mb-4">
        <div class="metric-card">
          <div class="metric-label">进程状态</div>
          <div class="metric-value"><StatusChip :status="data.status" /></div>
        </div>
        <div class="metric-card">
          <div class="metric-label">FRPC 版本</div>
          <div class="metric-value">{{ data.version || '未知' }}</div>
        </div>
        <div class="metric-card">
          <div class="metric-label">配置文件</div>
          <div class="metric-value">{{ data.configValid ? '有效' : '无效' }}</div>
        </div>
        <div class="metric-card">
          <div class="metric-label">代理数</div>
          <div class="metric-value">{{ data.proxyCount || 0 }}</div>
        </div>
        <div class="metric-card">
          <div class="metric-label">运行时长</div>
          <div class="metric-value">{{ data.uptime || '0s' }}</div>
        </div>
        <div class="metric-card">
          <div class="metric-label">守护策略</div>
          <div class="metric-value">{{ data.daemonPolicy || '默认' }}</div>
        </div>
      </div>

      <div class="grid-2">
        <div class="card">
          <div class="card-header">
            <h3 class="card-title">进程控制</h3>
          </div>
          <div class="card-body">
            <div class="mb-4">
              <span class="mr-2">当前状态:</span>
              <StatusChip :status="data.status" />
            </div>
            <div class="btn-group">
              <button class="btn btn-success" @click="processAction('start')" :disabled="actionLoading || data.status === 'running'">启动</button>
              <button class="btn btn-warning" @click="processAction('restart')" :disabled="actionLoading || data.status !== 'running'">重启</button>
              <button class="btn btn-danger" @click="processAction('stop')" :disabled="actionLoading || data.status !== 'running'">停止</button>
              <button class="btn btn-secondary" @click="openLogs">查看日志</button>
            </div>
          </div>
        </div>

        <div class="card">
          <div class="card-header">
            <h3 class="card-title">配置校验</h3>
          </div>
          <div class="card-body">
            <div class="kv-grid mb-3">
              <div class="kv-key">配置路径:</div>
              <div class="kv-val">{{ data.configPath }}</div>
              <div class="kv-key">状态:</div>
              <div class="kv-val"><StatusChip :status="data.configValid ? 'valid' : 'invalid'" /></div>
            </div>
            <div v-if="!data.configValid && data.configErrors" class="alert alert-danger">
              <div v-for="(err, i) in data.configErrors" :key="i">{{ err }}</div>
            </div>
            <button class="btn btn-secondary mt-2" @click="viewConfig">查看配置 (frpc.toml)</button>
          </div>
        </div>
      </div>
    </div>

    <Drawer title="运行日志" ref="logsDrawer" class="logs-drawer">
      <div class="toolbar mb-2">
        <label class="toggle-label">
          <input type="checkbox" v-model="autoRefreshLogs" @change="toggleAutoRefresh" /> 定时刷新
        </label>
        <button class="btn btn-sm btn-secondary" @click="fetchLogs">手动刷新</button>
      </div>
      <pre class="log-viewer">{{ logsContent }}</pre>
    </Drawer>

    <Drawer title="配置文件" ref="configDrawer">
      <pre class="log-viewer">{{ configContent }}</pre>
    </Drawer>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue';
import { get, post } from '@/api';
import { notify, openDrawer } from '@/stores/session';
import StatusChip from '@/components/StatusChip.vue';
import Drawer from '@/components/Drawer.vue';

const loading = ref(true);
const actionLoading = ref(false);
const data = ref<any>(null);

const logsDrawer = ref<any>(null);
const configDrawer = ref<any>(null);
const logsContent = ref('');
const configContent = ref('');
const autoRefreshLogs = ref(false);
let refreshTimer: any = null;

const load = async () => {
  loading.value = true;
  try {
    data.value = await get('/runtime');
  } catch (error: any) {
    notify(error.message || '加载运行时信息失败', 'error');
  } finally {
    loading.value = false;
  }
};

const processAction = async (action: string) => {
  actionLoading.value = true;
  try {
    await post('/runtime/action', { action });
    notify(`指令 ${action} 已下发`, 'success');
    await load();
  } catch (error: any) {
    notify(error.message || `操作失败`, 'error');
  } finally {
    actionLoading.value = false;
  }
};

const viewConfig = async () => {
  try {
    const config = await get('/runtime/config');
    configContent.value = config.content || JSON.stringify(config, null, 2);
    openDrawer('configDrawer');
  } catch (error: any) {
    notify(error.message || '加载配置失败', 'error');
  }
};

const fetchLogs = async () => {
  try {
    const res = await get('/runtime/logs?lines=500');
    logsContent.value = res.logs || res;
  } catch (error: any) {
    notify(error.message || '加载日志失败', 'error');
  }
};

const openLogs = async () => {
  await fetchLogs();
  openDrawer('logsDrawer');
};

const toggleAutoRefresh = () => {
  if (autoRefreshLogs.value) {
    refreshTimer = setInterval(fetchLogs, 3000);
  } else {
    if (refreshTimer) clearInterval(refreshTimer);
  }
};

onMounted(() => {
  load();
  if(logsDrawer.value) logsDrawer.value.id = 'logsDrawer';
  if(configDrawer.value) configDrawer.value.id = 'configDrawer';
});

onUnmounted(() => {
  if (refreshTimer) clearInterval(refreshTimer);
});

defineExpose({ load });
</script>
