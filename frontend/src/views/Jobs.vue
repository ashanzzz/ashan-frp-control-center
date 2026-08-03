<template>
  <div class="jobs-page">
    <div class="toolbar mb-4">
      <div class="toolbar-info">任务数量: {{ jobs.length }}</div>
      <div class="toolbar-actions">
        <button class="btn btn-secondary" @click="load" :disabled="loading">刷新</button>
      </div>
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
                <th>类型</th>
                <th>状态</th>
                <th>尝试次数</th>
                <th>错误信息</th>
                <th>操作</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="job in jobs" :key="job.id" :class="'status-' + job.status">
                <td>{{ formatDate(job.createdAt) }}</td>
                <td>{{ job.type }}</td>
                <td>
                  <StatusChip :status="job.status" />
                </td>
                <td>{{ job.attempts }}</td>
                <td class="text-truncate" style="max-width: 200px;" :title="job.error">{{ job.error || '-' }}</td>
                <td>
                  <button class="btn btn-sm btn-secondary mr-2" @click="viewJob(job.id)">详情</button>
                  <button class="btn btn-sm btn-primary mr-2" v-if="['failed', 'canceled'].includes(job.status)" @click="retryJob(job.id)">重试</button>
                  <button class="btn btn-sm btn-danger" v-if="['queued', 'retry_wait'].includes(job.status)" @click="cancelJob(job.id)">取消</button>
                </td>
              </tr>
              <tr v-if="jobs.length === 0">
                <td colspan="6" class="text-center text-muted">暂无任务</td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>
    </div>

    <Drawer title="任务详情" ref="jobDrawer">
      <div v-if="currentJob">
        <div class="kv-grid mb-4">
          <div class="kv-key">ID:</div>
          <div class="kv-val">{{ currentJob.id }}</div>
          <div class="kv-key">状态:</div>
          <div class="kv-val"><StatusChip :status="currentJob.status" /></div>
          <div class="kv-key">创建时间:</div>
          <div class="kv-val">{{ formatDate(currentJob.createdAt) }}</div>
          <div class="kv-key">更新时间:</div>
          <div class="kv-val">{{ formatDate(currentJob.updatedAt) }}</div>
        </div>
        <h4>数据 (Data)</h4>
        <pre class="log-viewer">{{ JSON.stringify(currentJob.data, null, 2) }}</pre>
        <h4 class="mt-4" v-if="currentJob.result">结果 (Result)</h4>
        <pre class="log-viewer" v-if="currentJob.result">{{ JSON.stringify(currentJob.result, null, 2) }}</pre>
      </div>
    </Drawer>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue';
import { get, post } from '@/api';
import { notify, openDrawer } from '@/stores/session';
import { formatDate } from '@/utils';
import StatusChip from '@/components/StatusChip.vue';
import Drawer from '@/components/Drawer.vue';

const loading = ref(true);
const jobs = ref<any[]>([]);
const currentJob = ref<any>(null);
const jobDrawer = ref<any>(null);

const load = async () => {
  loading.value = true;
  try {
    const data = await get('/jobs?limit=150');
    jobs.value = data || [];
  } catch (error: any) {
    notify(error.message || '加载任务列表失败', 'error');
  } finally {
    loading.value = false;
  }
};

const viewJob = async (id: string) => {
  try {
    currentJob.value = await get(`/jobs/${id}`);
    openDrawer('jobDrawer');
  } catch (error: any) {
    notify(error.message || '加载任务详情失败', 'error');
  }
};

const retryJob = async (id: string) => {
  try {
    await post(`/jobs/${id}/retry`);
    notify('任务重试已提交', 'success');
    await load();
  } catch (error: any) {
    notify(error.message || '重试失败', 'error');
  }
};

const cancelJob = async (id: string) => {
  if (!confirm('确定要取消此任务吗？')) return;
  try {
    await post(`/jobs/${id}/cancel`);
    notify('任务已取消', 'success');
    await load();
  } catch (error: any) {
    notify(error.message || '取消失败', 'error');
  }
};

onMounted(() => {
  load();
  if(jobDrawer.value) jobDrawer.value.id = 'jobDrawer';
});

defineExpose({ load });
</script>

<style scoped>
.status-running {
  background-color: rgba(13, 110, 253, 0.05);
}
.status-success {
  background-color: rgba(25, 135, 84, 0.05);
}
.status-failed {
  background-color: rgba(220, 53, 69, 0.05);
}
.status-retry_wait, .status-queued {
  background-color: rgba(255, 193, 7, 0.05);
}
</style>
