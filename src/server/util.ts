import { createHash, randomUUID } from 'node:crypto'

export const nowIso = () => new Date().toISOString()
export const id = () => randomUUID()
export const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))
export const sha256 = (value: string | Buffer) => createHash('sha256').update(value).digest('hex')
export function json<T>(value: unknown, fallback: T): T {
  if (value == null) return fallback
  if (typeof value !== 'string') return value as T
  try { return JSON.parse(value) as T } catch { return fallback }
}
export function clamp(value: number, min: number, max: number): number { return Math.min(max, Math.max(min, value)) }
export function unique<T>(items: T[]): T[] { return [...new Set(items)] }
export function text(value: unknown): string { return value == null ? '' : String(value).trim() }
export function numberValue(value: unknown, fallback = 0): number { const n = Number(value); return Number.isFinite(n) ? n : fallback }
export function boolValue(value: unknown, fallback = false): boolean {
  if (typeof value === 'boolean') return value
  if (value === 'true' || value === 1 || value === '1') return true
  if (value === 'false' || value === 0 || value === '0') return false
  return fallback
}
export function safeError(error: unknown): { code: string; message: string; details?: unknown } {
  const anyError = error as any
  return { code: text(anyError?.code) || 'INTERNAL_ERROR', message: error instanceof Error ? error.message : text(error) || '未知错误', details: anyError?.details }
}
export function redact(value: unknown): unknown {
  const secretPattern = /(token|secret|password|authorization|cookie|api[_-]?key|code)/i
  if (Array.isArray(value)) return value.map(redact)
  if (value && typeof value === 'object') {
    const out: Record<string, unknown> = {}
    for (const [key, val] of Object.entries(value as Record<string, unknown>)) out[key] = secretPattern.test(key) ? '[REDACTED]' : redact(val)
    return out
  }
  return value
}
export function maskSecret(value: string): string {
  if (!value) return ''
  if (value.length <= 8) return '••••••••'
  return `${value.slice(0, 3)}••••••••${value.slice(-4)}`
}
