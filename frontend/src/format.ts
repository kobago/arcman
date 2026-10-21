export function formatDuration(seconds: number): string {
  const total = Math.round(seconds)
  const h = Math.floor(total / 3600)
  const m = Math.floor((total % 3600) / 60)
  const s = String(total % 60).padStart(2, '0')
  return h > 0 ? `${h}:${String(m).padStart(2, '0')}:${s}` : `${m}:${s}`
}

export function formatDateTime(date: Date): string {
  const p = (n: number) => String(n).padStart(2, '0')
  return `${date.getFullYear()}-${p(date.getMonth() + 1)}-${p(date.getDate())} ${p(date.getHours())}:${p(date.getMinutes())}`
}

export function formatCount(n: number, locale: string): string {
  return n.toLocaleString(locale)
}

export function normalizeTagKey(name: string): string {
  return name.normalize('NFKC').trim().replace(/^#+/, '').trim().toLowerCase()
}

export type Swipe = 'left' | 'right' | 'up' | 'down' | null

export function swipeDirection(dx: number, dy: number, threshold = 50): Swipe {
  const ax = Math.abs(dx)
  const ay = Math.abs(dy)
  if (Math.max(ax, ay) < threshold) return null
  if (ax > ay * 1.5) return dx < 0 ? 'left' : 'right'
  if (ay > ax * 1.5) return dy < 0 ? 'up' : 'down'
  return null
}
