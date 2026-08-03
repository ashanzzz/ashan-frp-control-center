export function formatDate(value: string | null | undefined): string {
  if (!value) return '—'
  try {
    return new Intl.DateTimeFormat('zh-CN', {
      month: '2-digit', day: '2-digit',
      hour: '2-digit', minute: '2-digit', second: '2-digit',
    }).format(new Date(value))
  } catch { return '—' }
}

export function formatUptime(seconds: number): string {
  if (!seconds || seconds < 0) return '0s'
  const d = Math.floor(seconds / 86400)
  const h = Math.floor((seconds % 86400) / 3600)
  const m = Math.floor((seconds % 3600) / 60)
  const s = seconds % 60
  if (d > 0) return `${d}d ${h}h`
  if (h > 0) return `${h}h ${m}m`
  if (m > 0) return `${m}m ${s}s`
  return `${s}s`
}

export function statusClass(status: string): string {
  const s = String(status || '').toLowerCase()
  if (/healthy|online|running|success|succeeded|valid|fresh|configured|enabled|completed/.test(s)) return 'ok'
  if (/degraded|warning|pending|queued|retry|planned|medium|starting/.test(s)) return 'warn'
  if (/offline|failed|error|blocked|rollback|invalid|crashed|banned|disabled|stopped/.test(s)) return 'bad'
  return 'neutral'
}

export function formDataToObject(form: HTMLFormElement): Record<string, unknown> {
  const out: Record<string, unknown> = {}
  for (const [key, val] of new FormData(form).entries()) out[key] = val
  for (const input of form.querySelectorAll<HTMLInputElement>('input[type=checkbox]')) {
    out[input.name] = input.checked
  }
  return out
}
