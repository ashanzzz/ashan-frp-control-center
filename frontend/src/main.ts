import { createApp } from 'vue'
import { createRouter, createWebHashHistory } from 'vue-router'
import App from './App.vue'
import { appState } from './stores/session'
import './styles/global.css'

const routes = [
  { path: '/', redirect: '/dashboard' },
  { path: '/login', component: () => import('./views/Login.vue') },
  { path: '/dashboard', component: () => import('./views/Dashboard.vue') },
  { path: '/connections', component: () => import('./views/Connections.vue') },
  { path: '/auth', component: () => import('./views/AuthCenter.vue') },
  { path: '/nodes', component: () => import('./views/Nodes.vue') },
  { path: '/tunnels', component: () => import('./views/Tunnels.vue') },
  { path: '/dns', component: () => import('./views/Dns.vue') },
  { path: '/runtime', component: () => import('./views/Runtime.vue') },
  { path: '/automation', component: () => import('./views/Automation.vue') },
  { path: '/diagnostics', component: () => import('./views/Diagnostics.vue') },
  { path: '/jobs', component: () => import('./views/Jobs.vue') },
  { path: '/cache', component: () => import('./views/Cache.vue') },
  { path: '/audit', component: () => import('./views/Audit.vue') },
]

const router = createRouter({
  history: createWebHashHistory(),
  routes,
})

// Navigation guard: redirect unauthenticated users to login
router.beforeEach((to) => {
  if (to.path !== '/login' && !appState.session) {
    // Let App.vue handle init; if still no session after init, redirect
    return true
  }
})

const app = createApp(App)
app.use(router)
app.mount('#app')
