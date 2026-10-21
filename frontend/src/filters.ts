export type MediaFilter = '' | 'photo' | 'video' | 'text'

export type Filters = {
  folder: string
  q: string
  tags: number[]
  from: string
  to: string
  media: MediaFilter
  author: string
  order: 'desc' | 'asc'
}

const MEDIA: MediaFilter[] = ['photo', 'video', 'text']
const DATE = /^\d{4}-\d{2}-\d{2}$/

export function parseFilters(sp: URLSearchParams): Filters {
  const media = sp.get('media') as MediaFilter
  return {
    folder: sp.get('folder') || 'all',
    q: sp.get('q') ?? '',
    tags: (sp.get('tags') ?? '')
      .split(',')
      .map(Number)
      .filter((n) => Number.isInteger(n) && n > 0),
    from: DATE.test(sp.get('from') ?? '') ? sp.get('from')! : '',
    to: DATE.test(sp.get('to') ?? '') ? sp.get('to')! : '',
    media: MEDIA.includes(media) ? media : '',
    author: sp.get('author') ?? '',
    order: sp.get('order') === 'asc' ? 'asc' : 'desc',
  }
}

export function writeFilters(f: Filters): URLSearchParams {
  const sp = new URLSearchParams()
  if (f.folder !== 'all') sp.set('folder', f.folder)
  if (f.q) sp.set('q', f.q)
  if (f.tags.length) sp.set('tags', f.tags.join(','))
  if (f.from) sp.set('from', f.from)
  if (f.to) sp.set('to', f.to)
  if (f.media) sp.set('media', f.media)
  if (f.author) sp.set('author', f.author)
  if (f.order === 'asc') sp.set('order', 'asc')
  return sp
}

function localDay(date: string, offsetDays: number): string {
  const [y, m, d] = date.split('-').map(Number)
  return new Date(y, m - 1, d + offsetDays).toISOString()
}

export function toTileQuery(f: Filters, cursor: string | null, limit = 30): URLSearchParams {
  const sp = new URLSearchParams()
  if (f.folder !== 'all') sp.set('folder', f.folder)
  if (f.q.trim()) sp.set('q', f.q.trim())
  if (f.tags.length) sp.set('tags', f.tags.join(','))
  if (f.from) sp.set('from', localDay(f.from, 0))
  if (f.to) sp.set('to', localDay(f.to, 1))
  if (f.media) sp.set('media', f.media)
  if (f.author) sp.set('author', f.author)
  sp.set('order', f.order)
  if (cursor) sp.set('cursor', cursor)
  sp.set('limit', String(limit))
  return sp
}

export function folderIdOf(f: Filters): number | null {
  const id = Number(f.folder)
  return Number.isInteger(id) && id > 0 ? id : null
}
