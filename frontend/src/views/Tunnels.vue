<template>
  <div class="tunnels-page">
    <div class="toolbar mb-4">
      <div class="toolbar-info">期望: {{ desiredTunnels.length }} | 观测: {{ observedTunnels.length }}</div>
      <div class="toolbar-actions">
        <button class="btn btn-secondary" @click="syncRemote" :disabled="actionLoading">同步远端</button>
        <button class="btn btn-secondary" @click="previewDiff" :disabled="actionLoading">差异预览</button>
        <button class="btn btn-warning" @click="applyDiff" :disabled="actionLoading">应用差异</button>
        <button class="btn btn-primary" @click="openForm()" :disabled="actionLoading">新增隧道</button>
      </div>
    </div>

    <div v-if="loading" class="loading-state">
      <div class="spinner"></div>
      <p>加载中...</p>
    </div>

    <div v-else class="grid-2">
      <div class="card">
        <div class="card-header">
          <h3 class="card-title">期望状态</h3>
        </div>
        <div class="card-body">
          <div class="table-wrap">
            <table class="table">
              <thead>
                <tr>
                  <th>名称</th>
                  <th>协议</th>
                  <th>本地</th>
                  <th>远端</th>
                  <th>状态</th>
                  <th>操作</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="t in desiredTunnels" :key="t.id || t.name">
                  <td>{{ t.name }}</td>
                  <td>{{ t.protocol }}</td>
                  <td>{{ t.localIp }}:{{ t.localPort }}</td>
                  <td>{{ t.remotePort || t.customDomains }}</td>
                  <td><StatusChip :status="t.enabled ? 'active' : 'inactive'" /></td>
                  <td>
                    <button class="btn btn-sm btn-secondary" @click="openForm(t)">编辑</button>
                    <button class="btn btn-sm btn-danger ml-2" @click="deleteTunnel(t.id)">删除</button>
                  </td>
                </tr>
                <tr v-if="desiredTunnels.length === 0">
                  <td colspan="6" class="text-center text-muted">暂无数据</td>
                </tr>
              </tbody>
            </table>
          </div>
        </div>
      </div>

      <div class="card">
        <div class="card-header">
          <h3 class="card-title">观测状态 (远端)</h3>
        </div>
        <div class="card-body">
          <div class="table-wrap">
            <table class="table">
              <thead>
                <tr>
                  <th>名称</th>
                  <th>协议</th>
                  <th>状态</th>
                  <th>在线</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="t in observedTunnels" :key="t.name">
                  <td>{{ t.name }}</td>
                  <td>{{ t.protocol }}</td>
                  <td>{{ t.status }}</td>
                  <td><StatusChip :status="t.online ? 'online' : 'offline'" /></td>
                </tr>
                <tr v-if="observedTunnels.length === 0">
                  <td colspan="4" class="text-center text-muted">暂无数据</td>
                </tr>
              </tbody>
            </table>
          </div>
        </div>
      </div>
    </div>

    <Drawer title="隧道编辑" ref="formDrawer">
      <form @submit.prevent="saveTunnel" v-if="currentTunnel">
        <FormField label="名称" required>
          <input type="text" v-model="currentTunnel.name" required class="form-control" />
        </FormField>
        <FormField label="协议" required>
          <select v-model="currentTunnel.protocol" class="form-control" required>
            <option value="tcp">TCP</option>
            <option value="udp">UDP</option>
            <option value="http">HTTP</option>
            <option value="https">HTTPS</option>
          </select>
        </FormField>
        <FormField label="本地 IP" required>
          <input type="text" v-model="currentTunnel.localIp" required class="form-control" />
        </FormField>
        <FormField label="本地端口" required>
          <input type="number" v-model.number="currentTunnel.localPort" required class="form-control" />
        </FormField>
        <FormField label="远端端口" v-if="['tcp', 'udp'].includes(currentTunnel.protocol)">
          <input type="number" v-model.number="currentTunnel.remotePort" class="form-control" />
        </FormField>
        <FormField label="自定义域名" v-if="['http', 'https'].includes(currentTunnel.protocol)">
          <input type="text" v-model="currentTunnel.customDomains" class="form-control" />
        </FormField>
        <FormField label="健康检查 URL">
          <input type="text" v-model="currentTunnel.healthCheckUrl" class="form-control" />
        </FormField>
        <div class="toggle-field mt-3 mb-3">
          <label class="toggle-label">
            <input type="checkbox" v-model="currentTunnel.enabled" /> 启用
          </label>
        </div>
        <div class="form-actions mt-4">
          <button type="submit" class="btn btn-primary" :disabled="actionLoading">保存</button>
        </div>
      </form>
    </Drawer>

    <Drawer title="差异预览" ref="diffDrawer">
      <div v-if="diffData">
        <pre class="log-viewer">{{ JSON.stringify(diffData, null, 2) }}</pre>
      </div>
    </Drawer>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue';
import { get, post, put, del } from '@/api';
import { notify, openDrawer, closeDrawer } from '@/stores/session';
import StatusChip from '@/components/StatusChip.vue';
import Drawer from '@/components/Drawer.vue';
import FormField from '@/components/FormField.vue';

const loading = ref(true);
const actionLoading = ref(false);
const desiredTunnels = ref<any[]>([]);
const observedTunnels = ref<any[]>([]);

const formDrawer = ref<any>(null);
const diffDrawer = ref<any>(null);
const currentTunnel = ref<any>(null);
const diffData = ref<any>(null);

const load = async () => {
  loading.value = true;
  try {
    const data = await get('/tunnels');
    desiredTunnels.value = data?.desired || [];
    observedTunnels.value = data?.observed || [];
  } catch (error: any) {
    notify(error.message || '加载隧道数据失败', 'error');
  } finally {
    loading.value = false;
  }
};

const syncRemote = async () => {
  actionLoading.value = true;
  try {
    await post('/tunnels/sync');
    notify('远端同步完成', 'success');
    await load();
  } catch (error: any) {
    notify(error.message || '同步失败', 'error');
  } finally {
    actionLoading.value = false;
  }
};

const previewDiff = async () => {
  actionLoading.value = true;
  try {
    const diff = await post('/tunnels/plan');
    diffData.value = diff;
    openDrawer('diffDrawer');
  } catch (error: any) {
    notify(error.message || '生成差异失败', 'error');
  } finally {
    actionLoading.value = false;
  }
};

const applyDiff = async () => {
  if (!confirm('确定要应用差异配置吗？这会将变更加入队列执行。')) return;
  actionLoading.value = true;
  try {
    await post('/tunnels/apply');
    notify('配置应用已入队', 'success');
    await load();
  } catch (error: any) {
    notify(error.message || '应用配置失败', 'error');
  } finally {
    actionLoading.value = false;
  }
};

const openForm = (tunnel?: any) => {
  if (tunnel) {
    currentTunnel.value = { ...tunnel };
  } else {
    currentTunnel.value = {
      name: '',
      protocol: 'tcp',
      localIp: '127.0.0.1',
      localPort: 80,
      enabled: true
    };
  }
  openDrawer('formDrawer');
};

const saveTunnel = async () => {
  actionLoading.value = true;
  try {
    if (currentTunnel.value.id) {
      await put(`/tunnels/${currentTunnel.value.id}`, currentTunnel.value);
    } else {
      await post('/tunnels', currentTunnel.value);
    }
    notify('隧道保存成功', 'success');
    closeDrawer('formDrawer');
    await load();
  } catch (error: any) {
    notify(error.message || '保存失败', 'error');
  } finally {
    actionLoading.value = false;
  }
};

const deleteTunnel = async (id: string) => {
  if (!id || !confirm('确定要删除此隧道吗？')) return;
  actionLoading.value = true;
  try {
    await del(`/tunnels/${id}`);
    notify('隧道已删除', 'success');
    await load();
  } catch (error: any) {
    notify(error.message || '删除失败', 'error');
  } finally {
    actionLoading.value = false;
  }
};

onMounted(() => {
  load();
  if(formDrawer.value) formDrawer.value.id = 'formDrawer';
  if(diffDrawer.value) diffDrawer.value.id = 'diffDrawer';
});

defineExpose({ load });
</script>
