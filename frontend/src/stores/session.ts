import { reactive } from 'vue'

export interface User {
  id: string
  username: string
  displayName: string
}

export interface Session {
  user: User
  csrfToken: string
  expiresAt: string
  sessionId?: string
}

export interface Notification {
  id: number
  message: string
  type: 'success' | 'error' | 'warning' | 'info'
}

interface AppState {
  session: Session | null
  setupRequired: boolean
  notifications: Notification[]
  sseConnected: boolean
  drawerOpen: boolean
  drawerTitle: string
  drawerData: unknown
  drawerForm: string | null
}

let _notifId = 0

export const appState = reactive<AppState>({
  session: null,
  setupRequired: false,
  notifications: [],
  sseConnected: false,
  drawerOpen: false,
  drawerTitle: '',
  drawerData: null,
  drawerForm: null,
})

export function notify(message: string, type: Notification['type'] = 'success', duration = 3500) {
  const id = ++_notifId
  appState.notifications.push({ id, message, type })
  setTimeout(() => {
    const idx = appState.notifications.findIndex(n => n.id === id)
    if (idx !== -1) appState.notifications.splice(idx, 1)
  }, duration)
}

export function openDrawer(title: string, data: unknown, form: string | null = null) {
  appState.drawerTitle = title
  appState.drawerData = data
  appState.drawerForm = form
  appState.drawerOpen = true
}

export function closeDrawer() {
  appState.drawerOpen = false
  appState.drawerData = null
  appState.drawerForm = null
}
