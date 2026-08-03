<template>
  <!-- Auth pages (not logged in) -->
  <div v-if="!session && !loading" id="auth-root">
    <RouterView />
  </div>

  <!-- Loading -->
  <div v-else-if="loading" class="loading-state" style="height:100vh;background:var(--bg-base)">
    <div class="spinner"></div> 正在初始化…
  </div>

  <!-- Main Shell -->
  <div v-else class="shell">
    <!-- Sidebar -->
    <aside class="sidebar">
      <div class="brand">
        <div class="brand-logo">AF</div>
        <div class="brand-text">
          <div class="brand-title">Ashan FRP</div>
          <div class="brand-sub">HIGH AVAILABILITY</div>
        </div>
      </div>

      <nav class="sidebar-nav">
        <RouterLink
          v-for="item in navItems"
          :key="item.path"
          :to="item.path"
          class="nav-item"
          active-class="active"
        >
          <span class="nav-icon">{{ item.icon }}</span>
          <span class="nav-label">{{ item.label }}</span>
        </RouterLink>
      </nav>

      <div class="sidebar-footer">
        <span class="sidebar-user">{{ session?.user?.displayName || '管理员' }}</span>
        <button class="ghost" style="padding:4px 8px;font-size:12px" @click="logout">退出</button>
      </div>
    </aside>

    <!-- Main Content -->
    <div class="main-content">
      <!-- Topbar -->
      <div class="topbar">
        <div class="topbar-left">
          <div class="page-eyebrow">PERSONAL INFRA CONTROL PLANE</div>
          <div class="page-title">{{ currentPageTitle }}</div>
        </div>
        <div class="topbar-right">
          <div class="sse-badge">
            <div class="sse-dot" :class="sseState"></div>
            <span>{{ sseState === 'connected' ? '实时' : '离线' }}</span>
          </div>
          <button class="secondary" @click="refreshPage">⟳ 刷新</button>
        </div>
      </div>

      <!-- Page Body -->
      <div class="page-body">
        <RouterView v-slot="{ Component }">
          <Transition name="fade" mode="out-in">
            <component :is="Component" ref="pageRef" />
          </Transition>
        </RouterView>
      </div>
    </div>
  </div>

  <!-- Notifications -->
  <Teleport to="body">
    <div class="notif-list">
      <TransitionGroup name="notif">
        <div
          v-for="n in notifications"
          :key="n.id"
          class="notif"
          :class="n.type"
        >
          {{ n.message }}
        </div>
      </TransitionGroup>
    </div>
  </Teleport>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { appState, notify } from '@/stores/session'
import { get, post } from '@/api'

const route = useRoute()
const router = useRouter()
const loading = ref(true)
const sseState = ref<'connected' | 'error' | 'idle'>('idle')
const pageRef = ref<{ load?: () => void } | null>(null)

const session = computed(() => appState.session)
const notifications = computed(() => appState.notifications)

const navItems = [
  { path: '/dashboard',   icon: '⌂', label: '总览' },
  { path: '/connections', icon: '⌁', label: '连接中心' },
  { path: '/auth',        icon: '◉', label: '认证中心' },
  { path: '/nodes',       icon: '◈', label: '节点与切换' },
  { path: '/tunnels',     icon: '⇄', label: '隧道对账' },
  { path: '/dns',         icon: '☁', label: 'DNS 对账' },
  { path: '/runtime',     icon: '⚙', label: 'FRPC 运行时' },
  { path: '/automation',  icon: '⏱', label: '自动化策略' },
  { path: '/diagnostics', icon: '⌘', label: 'API 诊断' },
  { path: '/jobs',        icon: '▤', label: '任务中心' },
  { path: '/cache',       icon: '▦', label: '缓存与快照' },
  { path: '/audit',       icon: '◎', label: '审计日志' },
]

const currentPageTitle = computed(() => {
  const match = navItems.find(n => route.path.startsWith(n.path))
  return match?.label || '控制中心'
})

let es: EventSource | null = null

function connectSSE() {
  if (es) return
  try {
    es = new EventSource('/api/v1/events')
    es.addEventListener('open', () => { sseState.value = 'connected'; appState.sseConnected = true })
    es.addEventListener('job.updated', () => { pageRef.value?.load?.() })
    es.addEventListener('runtime.started', () => { pageRef.value?.load?.() })
    es.addEventListener('runtime.exited', () => { pageRef.value?.load?.() })
    es.onerror = () => { sseState.value = 'error'; appState.sseConnected = false }
  } catch { /* ignore */ }
}

async function init() {
  loading.value = true
  try {
    const status = await fetch('/api/v1/auth/status').then(r => r.json())
    if (status.data?.authenticated) {
      appState.session = await get<typeof appState.session>('/auth/session')
      connectSSE()
    } else if (status.data?.setupRequired) {
      await router.replace('/dashboard')  // will show setup in RouterView
      appState.setupRequired = true
      appState.session = null
    } else {
      await router.replace('/dashboard')
      appState.session = null
    }
  } catch (e) {
    console.error('init failed', e)
  } finally {
    loading.value = false
  }
}

async function logout() {
  try { await post('/auth/logout') } catch { /* ignore */ }
  appState.session = null
  location.reload()
}

function refreshPage() {
  pageRef.value?.load?.()
}

onMounted(init)
onUnmounted(() => { es?.close() })
</script>

<style scoped>
.fade-enter-active, .fade-leave-active { transition: opacity 0.15s ease; }
.fade-enter-from, .fade-leave-to { opacity: 0; }

.notif-enter-active { animation: slideIn 0.2s ease; }
.notif-leave-active { transition: all 0.2s ease; }
.notif-leave-to { transform: translateX(100%); opacity: 0; }

#auth-root {
  min-height: 100vh;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--bg-base);
}
</style>
