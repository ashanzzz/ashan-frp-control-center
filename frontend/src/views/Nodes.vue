<template>
  <div class="nodes-page">
    <div class="toolbar mb-4">
      <div class="toolbar-info">节点数量: {{ nodes.length }}</div>
      <div class="toolbar-actions">
        <button class="btn btn-secondary" @click="syncNodes" :disabled="actionLoading">同步测速</button>
        <button class="btn btn-primary" @click="generatePlan" :disabled="actionLoading">生成最优切换计划</button>
      </div>
    </div>

    <div v-if="loading" class="loading-state">
      <div class="spinner"></div>
      <p>加载中...</p>
    </div>

    <div v-else class="node-grid">
      <div class="node-card" :class="{ current: node.isCurrent }" v-for="node in nodes" :key="node.name">
        <div class="node-card-head">
          <div class="node-name">{{ node.name }}</div>
          <StatusChip :status="node.status" />
        </div>
        <div class="node-meta mb-2">
          <div>地区: {{ node.region }}</div>
          <div>地址: {{ node.ip }}:{{ node.port }}</div>
          <div class="node-latency" :class="{ 'text-danger': node.latency > 150 }">
            延迟: {{ node.latency }}ms (丢包: {{ node.lossRate }}%)
          </div>
          <div>评分: {{ node.score }}</div>
          <div v-if="node.banned" class="text-danger">已封禁</div>
        </div>
        <div class="node-actions">
          <button class="btn btn-sm btn-secondary" @click="testNode(node.name)" :disabled="actionLoading">测试</button>
          <button v-if="node.banned" class="btn btn-sm btn-warning" @click="unbanNode(node.name)" :disabled="actionLoading">解封</button>
          <button class="btn btn-sm btn-primary" @click="switchNode(node.name)" :disabled="actionLoading || node.isCurrent">切换计划</button>
        </div>
      </div>
    </div>

    <div class="card mt-4" v-if="!loading && switchPlans.length > 0">
      <div class="card-header">
        <h3 class="card-title">切换计划</h3>
      </div>
      <div class="card-body">
        <div class="table-wrap">
          <table class="table">
            <thead>
              <tr>
                <th>ID</th>
                <th>源节点</th>
                <th>目标节点</th>
                <th>状态</th>
                <th>创建时间</th>
                <th>操作</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="plan in switchPlans" :key="plan.id">
                <td>{{ plan.id }}</td>
                <td>{{ plan.sourceNode }}</td>
                <td>{{ plan.targetNode }}</td>
                <td><StatusChip :status="plan.status" /></td>
                <td>{{ formatDate(plan.createdAt) }}</td>
                <td>
                  <button class="btn btn-sm btn-primary" @click="executePlan(plan.id)" :disabled="actionLoading || plan.status !== 'pending'">执行</button>
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue';
import { get, post, del } from '@/api';
import { notify } from '@/stores/session';
import { formatDate } from '@/utils';
import StatusChip from '@/components/StatusChip.vue';

const loading = ref(true);
const actionLoading = ref(false);
const nodes = ref<any[]>([]);
const switchPlans = ref<any[]>([]);

const load = async () => {
  loading.value = true;
  try {
    const [nodesData, plansData] = await Promise.all([
      get('/nodes'),
      get('/switch-plans')
    ]);
    nodes.value = nodesData || [];
    switchPlans.value = plansData || [];
  } catch (error: any) {
    notify(error.message || '加载节点数据失败', 'error');
  } finally {
    loading.value = false;
  }
};

const syncNodes = async () => {
  actionLoading.value = true;
  try {
    await post('/nodes/sync');
    notify('节点同步已触发', 'success');
    await load();
  } catch (error: any) {
    notify(error.message || '同步失败', 'error');
  } finally {
    actionLoading.value = false;
  }
};

const generatePlan = async () => {
  actionLoading.value = true;
  try {
    await post('/switch-plans');
    notify('已生成切换计划', 'success');
    await load();
  } catch (error: any) {
    notify(error.message || '生成计划失败', 'error');
  } finally {
    actionLoading.value = false;
  }
};

const testNode = async (name: string) => {
  actionLoading.value = true;
  try {
    await post(`/nodes/${name}/test`);
    notify('测试已触发', 'success');
    await load();
  } catch (error: any) {
    notify(error.message || '测试失败', 'error');
  } finally {
    actionLoading.value = false;
  }
};

const unbanNode = async (name: string) => {
  actionLoading.value = true;
  try {
    await del(`/nodes/${name}/ban`);
    notify('节点已解封', 'success');
    await load();
  } catch (error: any) {
    notify(error.message || '解封失败', 'error');
  } finally {
    actionLoading.value = false;
  }
};

const switchNode = async (name: string) => {
  actionLoading.value = true;
  try {
    await post(`/nodes/${name}/switch-plan`);
    notify('已创建到该节点的切换计划', 'success');
    await load();
  } catch (error: any) {
    notify(error.message || '操作失败', 'error');
  } finally {
    actionLoading.value = false;
  }
};

const executePlan = async (id: string) => {
  if (!confirm('确定要执行此切换计划吗？')) return;
  actionLoading.value = true;
  try {
    await post(`/switch-plans/${id}/execute`);
    notify('计划执行已入队', 'success');
    await load();
  } catch (error: any) {
    notify(error.message || '执行计划失败', 'error');
  } finally {
    actionLoading.value = false;
  }
};

onMounted(() => {
  load();
});

defineExpose({ load });
</script>
