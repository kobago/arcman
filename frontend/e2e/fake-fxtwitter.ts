const PORT = 3191
const base = `http://127.0.0.1:${PORT}`
const file = (name: string) => `${base}/files/${name}`
const png = Uint8Array.from(
  atob('iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAIAAAD91JpzAAAAEElEQVR4nGOwcgsDIgYIBQAXBgNZRw9UswAAAABJRU5ErkJggg=='),
  (c) => c.charCodeAt(0),
)

const author = (screen_name: string, name: string) => ({ screen_name, name, avatar_url: file(`${screen_name}.png`) })
const photos = (id: string, n: number) =>
  Array.from({ length: n }, (_, i) => ({ type: 'photo', url: file(`${id}-${i}.png`), width: i % 2 ? 600 : 900, height: 600 }))

function status(id: string, text: string, extra: Record<string, unknown> = {}) {
  return {
    type: 'status',
    id,
    url: `https://x.com/alice/status/${id}`,
    text,
    lang: 'ja',
    created_timestamp: 1_760_000_000,
    author: author('alice', 'Alice'),
    likes: 12,
    reposts: 3,
    replies: 1,
    views: 456,
    media: {},
    ...extra,
  }
}

const statuses: Record<string, unknown> = {
  '1001': status('1001', 'two photos', { media: { all: photos('1001', 2) } }),
  '1003': status('1003', 'Morning light over the harbor.', {
    lang: 'en',
    translation: { text: '港に差す朝の光。', source_lang: 'en' },
    quote: { type: 'tombstone', reason: 'deleted', message: 'This post was deleted' },
    card: { url: 'https://example.com/a', title: '港町の朝を歩く', domain: 'example.com', image: { url: file('card.png') } },
  }),
  '1004': status('1004', 'folder post', { media: { all: photos('1004', 1) } }),
  '1005': status('1005', 'tag me please'),
  '1006': status('1006', 'three photos', { media: { all: photos('1006', 3) } }),
  '1007': status('1007', 'mobile photos', { media: { all: photos('1007', 2) } }),
  '1008': status('1008', 'a short clip', {
    media: {
      all: [{ type: 'video', url: file('1008.mp4'), thumbnail_url: file('1008-thumb.png'), width: 1280, height: 720, duration: 24.6, formats: [{ url: file('1008.mp4'), container: 'mp4', bitrate: 832000 }] }],
    },
  }),
}

const videos: Record<string, unknown> = {
  dQw4w9WgXcQ: {
    title: '港町を歩く動画',
    author_name: 'Harbor Walks',
    author_url: 'https://www.youtube.com/@harborwalks',
    type: 'video',
    thumbnail_url: `${base}/vi/dQw4w9WgXcQ/hqdefault.jpg`,
  },
}

Bun.serve({
  port: PORT,
  hostname: '127.0.0.1',
  fetch(req) {
    const { pathname } = new URL(req.url)
    if (pathname === '/health') return new Response('ok')
    const statusMatch = pathname.match(/^\/2\/status\/(\d+)$/)
    if (statusMatch) {
      const found = statuses[statusMatch[1]]
      return Response.json({ code: found ? 200 : 404, status: found ?? null }, { status: found ? 200 : 404 })
    }
    if (pathname === '/oembed') {
      const watch = new URL(new URL(req.url).searchParams.get('url') ?? '')
      const found = videos[watch.searchParams.get('v') ?? '']
      return found ? Response.json(found) : new Response('Not Found', { status: 404 })
    }
    if (pathname.startsWith('/files/') || pathname.startsWith('/vi/')) {
      const isVideo = pathname.endsWith('.mp4')
      return new Response(isVideo ? new Uint8Array(64) : png, { headers: { 'Content-Type': isVideo ? 'video/mp4' : 'image/png' } })
    }
    return new Response('not found', { status: 404 })
  },
})

console.log(`fake fxtwitter on ${base}`)
