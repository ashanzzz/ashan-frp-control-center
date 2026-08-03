<template>
  <div>
    <div v-if="loading" class="loading-state"><div class="spinner"></div> 加载中…</div>
    <template v-else-if="!session">
      <div class="auth-page">
        <div class="auth-card">
          <div class="auth-logo">AF</div>
          <div class="auth-eyebrow">ASHAN FRP CONTROL PLANE</div>
          <h1 class="auth-h1">{{ setupRequired ? '创建管理员账户' : '登录控制中心' }}</h1>
          <p class="auth-hint">{{ setupRequired ? '首次启动，请创建唯一管理员账户。' : '使用本地管理员账户登录。' }}</p>
          <form class="form" @submit.prevent="submit">
            <div class="form-field">
              <label class="form-label">用户名</label>
              <input v-model="form.username" type="text" placeholder="admin" required />
            </div>
            <div class="form-field" v-if="setupRequired">
              <label class="form-label">显示名称</label>
              <input v-model="form.displayName" type="text" placeholder="Ashan" />
            </div>
            <div class="form-field">
              <label class="form-label">密码</label>
              <input v-model="form.password" type="password" placeholder="••••••••••" required />
            </div>
            <button class="primary" type="submit" :disabled="submitting" style="width:100%;justify-content:center">
              {{ submitting ? '请稍候…' : (setupRequired ? '完成初始化' : '登录') }}
            </button>
          </form>
          <p class="auth-hint" style="font-size:11px">凭据加密保存于本地 SQLite，密钥不写入浏览器存储</p>
        </div>
      </div>
    </template>
    <template v-else>
      <!-- Authenticated: redirect handled by App.vue -->
    </template>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { appState, notify } from '@/stores/session'
import { get } from '@/api'

const router = useRouter()
const loading = ref(true)
const submitting = ref(false)
const session = computed(() => appState.session)
const setupRequired = computed(() => appState.setupRequired)
const form = ref({ username: '', displayName: 'Ashan', password: '' })

async function load() {
  loading.value = true
  try {
    const status = await fetch('/api/v1/auth/status').then(r => r.json())
    appState.setupRequired = !!status.data?.setupRequired
    if (status.data?.authenticated) {
      appState.session = await get<typeof appState.session>('/auth/session')
      router.replace('/dashboard')
    }
  } catch { /* ignore */ } finally { loading.value = false }
}

async function submit() {
  submitting.value = true
  try {
    const endpoint = setupRequired.value ? '/api/v1/auth/setup' : '/api/v1/auth/login'
    const res = await fetch(endpoint, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(form.value),
    }).then(r => r.json())
    if (!res.ok) throw new Error(res.error?.message || '登录失败')
    appState.session = await get<typeof appState.session>('/auth/session')
    router.replace('/dashboard')
  } catch (e: any) {
    notify(e.message || '登录失败', 'error')
  } finally {
    submitting.value = false
  }
}

onMounted(load)
defineExpose({ load })
</script>
