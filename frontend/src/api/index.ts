import { appState } from '@/stores/session'

class ApiError extends Error {
  code?: string
  details?: unknown
  status?: number
  constructor(message: string, code?: string, details?: unknown, status?: number) {
    super(message)
    this.code = code
    this.details = details
    this.status = status
  }
}

export async function api<T = unknown>(
  path: string,
  options: RequestInit = {}
): Promise<T> {
  const headers: Record<string, string> = {
    accept: 'application/json',
    ...(options.headers as Record<string, string> || {}),
  }
  if (options.body && !headers['content-type']) {
    headers['content-type'] = 'application/json'
  }
  const method = String(options.method || 'GET').toUpperCase()
  if (!['GET', 'HEAD'].includes(method) && appState.session?.csrfToken) {
    headers['x-csrf-token'] = appState.session.csrfToken
  }
  const res = await fetch(`/api/v1${path}`, {
    credentials: 'same-origin',
    ...options,
    headers,
  })
  let payload: { ok: boolean; data?: T; error?: { message: string; code?: string; details?: unknown } }
  try { payload = await res.json() } catch {
    throw new ApiError(`HTTP ${res.status}`, 'HTTP_ERROR', null, res.status)
  }
  if (!res.ok || !payload.ok) {
    const e = payload.error
    throw new ApiError(e?.message || `HTTP ${res.status}`, e?.code, e?.details, res.status)
  }
  return payload.data as T
}

export const get = <T = unknown>(path: string) => api<T>(path)
export const post = <T = unknown>(path: string, body: unknown = {}) =>
  api<T>(path, { method: 'POST', body: JSON.stringify(body) })
export const put = <T = unknown>(path: string, body: unknown = {}) =>
  api<T>(path, { method: 'PUT', body: JSON.stringify(body) })
export const patch = <T = unknown>(path: string, body: unknown = {}) =>
  api<T>(path, { method: 'PATCH', body: JSON.stringify(body) })
export const del = <T = unknown>(path: string) => api<T>(path, { method: 'DELETE' })

export type { ApiError }
