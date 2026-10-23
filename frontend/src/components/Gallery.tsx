import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { Link } from 'react-router'
import { api, type Tile } from '../api'
import { formatDuration } from '../format'
import { rowHeight } from '../gallerySize'
import { useI18n } from '../i18n'
import { aspectRatio, justify } from '../layout'
import { Icon, PlayGlyph } from './Icon'

type Props = {
  tiles: Tile[]
  search: string
  hasMore: boolean
  loadingMore: boolean
  onLoadMore: () => void
  size: number
}

const tileKey = (t: Pick<Tile, 'post_id' | 'index'>) => `${t.post_id}-${t.index}`

function useWidth() {
  const ref = useRef<HTMLDivElement>(null)
  const [width, setWidth] = useState(0)
  useLayoutEffect(() => {
    const el = ref.current
    if (!el) return
    const observer = new ResizeObserver(([entry]) => setWidth(Math.floor(entry.contentRect.width)))
    observer.observe(el)
    setWidth(Math.floor(el.getBoundingClientRect().width))
    return () => observer.disconnect()
  }, [])
  return [ref, width] as const
}

function hostOf(url: string) {
  try {
    return new URL(url).host.replace(/^www\./, '')
  } catch {
    return url
  }
}

function TileBody({ tile }: { tile: Tile }) {
  const { t } = useI18n()
  const qc = useQueryClient()
  const retry = useMutation({
    mutationFn: () => api.retryPost(tile.post_id),
    onSettled: () => qc.invalidateQueries({ queryKey: ['tiles'] }),
  })
  switch (tile.kind) {
    case 'photo':
      return <img src={tile.src} alt="" loading="lazy" decoding="async" />
    case 'gif':
      return <video src={tile.src} poster={tile.thumbnail} autoPlay loop muted playsInline preload="metadata" />
    case 'video':
      return (
        <>
          <img src={tile.thumbnail} alt="" loading="lazy" decoding="async" />
          <span className="badge">
            <PlayGlyph />
            {tile.duration ? formatDuration(tile.duration) : null}
          </span>
        </>
      )
    case 'youtube':
      return (
        <>
          <img src={tile.src} alt="" loading="lazy" decoding="async" />
          <span className="badge">
            <Icon name="youtube" size={14} strokeWidth={2} />
          </span>
          <span className="tile-caption ellipsis">{tile.text}</span>
        </>
      )
    case 'web': {
      const [title, ...rest] = (tile.text ?? '').split('\n')
      return tile.src ? (
        <>
          <img src={tile.src} alt="" loading="lazy" decoding="async" />
          <span className="badge">
            <Icon name="link" size={13} strokeWidth={2} />
          </span>
          <span className="tile-caption ellipsis">{title}</span>
        </>
      ) : (
        <div className="tile-text">
          <p>
            <span className="strong">{title}</span>
            {rest.length > 0 && `\n${rest.join('\n')}`}
          </p>
          <span className="tile-author ellipsis">
            <Icon name="link" size={12} strokeWidth={2} /> {tile.author_name}
          </span>
        </div>
      )
    }
    case 'text':
      return (
        <div className="tile-text">
          <p>{tile.text}</p>
          <span className="tile-author ellipsis">
            {tile.author_name} <span className="muted">@{tile.author_screen_name}</span>
          </span>
        </div>
      )
    case 'pending':
      return (
        <div className="tile-state">
          <span className="spinner" aria-hidden="true" />
          {t('tile.pending')}
        </div>
      )
    case 'failed':
      return (
        <div className="tile-state failed">
          <span className="danger-text">{t('tile.failed')}</span>
          {tile.failure && <span className="muted small">{t(`failure.${tile.failure}` as 'failure.api_unavailable')}</span>}
          <span className="muted small ellipsis tile-host">{hostOf(tile.url)}</span>
          <button
            type="button"
            className="btn btn-sm"
            disabled={retry.isPending}
            onClick={(e) => {
              e.preventDefault()
              e.stopPropagation()
              retry.mutate()
            }}
          >
            <Icon name="retry" size={13} strokeWidth={2} /> {t('common.retry')}
          </button>
        </div>
      )
  }
}

export function Gallery({ tiles, search, hasMore, loadingMore, onLoadMore, size }: Props) {
  const { t } = useI18n()
  const [ref, width] = useWidth()
  const sentinel = useRef<HTMLDivElement>(null)
  const narrow = width > 0 && width < 600
  const gap = narrow ? 2 : 4
  const rows = width ? justify(tiles, (x) => aspectRatio(x.width, x.height), width, rowHeight(size, narrow), gap) : []

  useEffect(() => {
    const el = sentinel.current
    if (!el || !hasMore) return
    const observer = new IntersectionObserver(([entry]) => {
      if (entry.isIntersecting && !loadingMore) onLoadMore()
    }, { rootMargin: '800px' })
    observer.observe(el)
    return () => observer.disconnect()
  }, [hasMore, loadingMore, onLoadMore])

  return (
    <div className="gallery" ref={ref} style={{ gap }}>
      {rows.map((row, i) => (
        <div className="gallery-row" key={i} style={{ gap, height: row.height }}>
          {row.tiles.map(({ item, width: w }) => (
            <Link
              key={tileKey(item)}
              to={`/posts/${item.post_id}/${item.index}${search}`}
              className={`tile ${item.kind}`}
              style={{ width: w, height: row.height }}
              aria-label={item.count > 1 ? `${t('tile.open')} (${item.index + 1}/${item.count})` : t('tile.open')}
              data-testid="tile"
            >
              <TileBody tile={item} />
              {item.count > 1 && item.kind !== 'video' && (
                <span className="badge">
                  {item.index + 1}/{item.count}
                </span>
              )}
            </Link>
          ))}
        </div>
      ))}
      <div ref={sentinel} className="sentinel" aria-hidden="true" />
    </div>
  )
}
