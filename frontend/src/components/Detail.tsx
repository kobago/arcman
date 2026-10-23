import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useCallback, useEffect, useRef, useState, type MouseEvent, type PointerEvent } from 'react'
import { useLocation, useNavigate, useOutletContext, useParams } from 'react-router'
import { api, type MediaView, type PostDetail, type StatusView, type Tile, type VideoView } from '../api'
import { formatCount, formatDateTime, swipeDirection } from '../format'
import { errorMessage, useI18n } from '../i18n'
import { ConfirmDialog } from './ConfirmDialog'
import { Icon } from './Icon'
import { TagInput } from './TagInput'

export type GalleryContext = { tiles: Tile[]; hasMore: boolean; loadMore: () => void }

function Media({ media }: { media: MediaView }) {
  const ref = useRef<HTMLVideoElement>(null)
  useEffect(() => {
    const video = ref.current
    if (!video || media.kind !== 'video') return
    video.play().catch(() => {
      video.muted = true
      video.play().catch(() => {})
    })
  }, [media])
  if (media.kind === 'photo') return <img className="stage-media" src={media.src} alt="" draggable={false} />
  if (media.kind === 'gif') return <video className="stage-media" src={media.src} autoPlay loop muted playsInline />
  return <video ref={ref} key={media.src} className="stage-media" src={media.src} poster={media.thumbnail} controls playsInline />
}

function TilePreview({ tile }: { tile: Tile }) {
  if (tile.kind === 'photo' || tile.kind === 'youtube') return <img className="stage-media" src={tile.src} alt="" draggable={false} />
  if (tile.kind === 'video' || tile.kind === 'gif') return <img className="stage-media" src={tile.thumbnail} alt="" draggable={false} />
  if (tile.kind === 'text') return <div className="stage-text">{tile.text}</div>
  return null
}

function YoutubePlayer({ video }: { video: VideoView }) {
  return (
    <iframe
      key={video.id}
      className={video.shorts ? 'stage-player shorts' : 'stage-player'}
      src={`https://www.youtube-nocookie.com/embed/${video.id}?autoplay=1&playsinline=1&rel=0`}
      title={video.title}
      allow="autoplay; encrypted-media; picture-in-picture; fullscreen"
      allowFullScreen
    />
  )
}

function VideoInfo({ video }: { video: VideoView }) {
  const { t } = useI18n()
  return (
    <>
      <div className="author">
        <div className="author-names">
          <a className="strong ellipsis" href={video.channel.url} target="_blank" rel="noreferrer">
            {video.channel.name}
          </a>
        </div>
        <a href={video.url} target="_blank" rel="noreferrer" className="link-small">
          {t('detail.openOnYoutube')} <Icon name="external" size={13} strokeWidth={2} />
        </a>
      </div>
      <p className="post-text">{video.title}</p>
    </>
  )
}

function Status({ status, nested }: { status: StatusView; nested?: boolean }) {
  const { t, lang } = useI18n()
  const locale = lang === 'ja' ? 'ja-JP' : 'en-US'
  const blocks = (status.article?.content as { blocks?: { text?: string }[] } | undefined)?.blocks ?? []
  return (
    <>
      <div className="author">
        {status.author.avatar ? <img className="avatar" src={status.author.avatar} alt="" /> : <span className="avatar" />}
        <div className="author-names">
          <span className="strong ellipsis">{status.author.name}</span>
          <span className="muted small">@{status.author.screen_name}</span>
        </div>
        {!nested && (
          <a href={status.url} target="_blank" rel="noreferrer" className="link-small">
            {t('detail.openOnX')} <Icon name="external" size={13} strokeWidth={2} />
          </a>
        )}
      </div>
      {status.text && <p className="post-text">{status.text}</p>}
      {status.translation?.text && status.lang !== 'ja' && (
        <div className="translation">
          <span className="muted small">{t('detail.translation')}</span>
          <span>{status.translation.text}</span>
        </div>
      )}
      {status.article && (
        <article className="x-article">
          <span className="muted small">{t('detail.article')}</span>
          {status.article.content.title && <h3>{status.article.content.title}</h3>}
          {blocks.length
            ? blocks.map((b, i) => (b.text ? <p key={i}>{b.text}</p> : null))
            : status.article.content.preview_text && <p>{status.article.content.preview_text}</p>}
          {Object.values(status.article.local_urls).map((src) => (
            <img key={src} src={src} alt="" loading="lazy" />
          ))}
        </article>
      )}
      {nested && status.media.length > 0 && (
        <div className="quote-media">
          {status.media.map((m) => (
            <img key={m.src} src={m.thumbnail ?? m.src} alt="" />
          ))}
        </div>
      )}
      {status.quote && (
        <div className="quote">
          {status.quote.available && status.quote.status ? (
            <Status status={status.quote.status} nested />
          ) : (
            <p className="muted small quote-missing">
              <Icon name="info" /> {t('detail.quoteUnavailable')}
              {status.quote.message ? `（${status.quote.message}）` : ''}
            </p>
          )}
        </div>
      )}
      {status.card && (
        <a className="card" href={status.card.url} target="_blank" rel="noreferrer">
          {status.card.image && <img src={status.card.image} alt="" />}
          <span className="card-body">
            {status.card.domain && <span className="muted small">{status.card.domain}</span>}
            {status.card.title && <span>{status.card.title}</span>}
          </span>
        </a>
      )}
      {status.poll?.choices && (
        <div className="poll" aria-label={t('detail.poll')}>
          {status.poll.choices.map((c) => (
            <div key={c.label} className="poll-choice">
              <span className="poll-bar" style={{ width: `${c.percentage}%` }} />
              <span>{c.label}</span>
              <span className="mono">{c.percentage}%</span>
            </div>
          ))}
          {status.poll.total_votes !== undefined && (
            <span className="muted small">{t('detail.votes', { count: formatCount(status.poll.total_votes, locale) })}</span>
          )}
        </div>
      )}
      {status.community_note && (
        <div className="note-box">
          <span className="muted small">{t('detail.note')}</span>
          <span>{status.community_note.text}</span>
        </div>
      )}
      {!nested && (
        <div className="stats">
          {(['replies', 'reposts', 'quotes', 'likes', 'views', 'bookmarks'] as const).map((k) =>
            status.stats[k] === null ? null : (
              <span key={k}>
                {t(`detail.${k}`)} <b className="mono">{formatCount(status.stats[k]!, locale)}</b>
              </span>
            ),
          )}
        </div>
      )}
    </>
  )
}

function Panel({ post }: { post: PostDetail }) {
  const { t } = useI18n()
  const qc = useQueryClient()
  const navigate = useNavigate()
  const location = useLocation()
  const [confirmDelete, setConfirmDelete] = useState(false)
  const sidebar = useQuery({ queryKey: ['sidebar'], queryFn: api.sidebar })
  const refresh = () => {
    qc.invalidateQueries({ queryKey: ['post', post.id] })
    qc.invalidateQueries({ queryKey: ['tiles'] })
  }
  const move = useMutation({ mutationFn: (folderId: number | null) => api.movePost(post.id, folderId), onSettled: refresh })
  const tags = useMutation({
    mutationFn: (names: string[]) => api.setPostTags(post.id, names),
    onSuccess: (next) => qc.setQueryData<PostDetail>(['post', post.id], (p) => (p ? { ...p, tags: next } : p)),
    onSettled: () => (qc.invalidateQueries({ queryKey: ['tags'] }), qc.invalidateQueries({ queryKey: ['tiles'] })),
  })
  const remove = useMutation({
    mutationFn: () => api.deletePost(post.id),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['tiles'] })
      qc.removeQueries({ queryKey: ['post', post.id] })
      navigate(`/${location.search}`)
    },
  })
  const error = move.error ?? tags.error ?? remove.error

  return (
    <>
      {post.status && <Status status={post.status} />}
      {post.video && <VideoInfo video={post.video} />}
      <dl className="dates">
        {post.status?.posted_at && (
          <>
            <dt>{t('detail.postedAt')}</dt>
            <dd className="mono">{formatDateTime(new Date(post.status.posted_at * 1000))}</dd>
          </>
        )}
        <dt>{t('detail.savedAt')}</dt>
        <dd className="mono">{formatDateTime(new Date(post.saved_at))}</dd>
      </dl>
      <hr />
      <label className="field-label" htmlFor="post-folder">
        {t('detail.folder')}
      </label>
      <select
        id="post-folder"
        className="input"
        value={post.folder_id ?? ''}
        disabled={move.isPending}
        onChange={(e) => move.mutate(e.target.value ? Number(e.target.value) : null)}
      >
        <option value="">{t('nav.unsorted')}</option>
        {sidebar.data?.categories.map((c) => (
          <optgroup key={c.id} label={c.name}>
            {c.folders.map((f) => (
              <option key={f.id} value={f.id}>
                {c.name} / {f.name}
              </option>
            ))}
          </optgroup>
        ))}
        {!!sidebar.data?.uncategorized.length && (
          <optgroup label={t('nav.uncategorized')}>
            {sidebar.data.uncategorized.map((f) => (
              <option key={f.id} value={f.id}>
                {f.name}
              </option>
            ))}
          </optgroup>
        )}
      </select>
      <span className="field-label">{t('detail.tags')}</span>
      <TagInput value={post.tags} busy={tags.isPending} onChange={(names) => tags.mutate(names)} />
      {error && (
        <p className="form-error" role="alert">
          {errorMessage(t, error)}
        </p>
      )}
      <span className="spacer" />
      <button type="button" className="btn btn-danger" onClick={() => setConfirmDelete(true)}>
        <Icon name="trash" size={15} /> {t('detail.delete')}
      </button>
      {confirmDelete && (
        <ConfirmDialog
          title={t('detail.deleteTitle')}
          confirmLabel={t('common.delete')}
          danger
          busy={remove.isPending}
          onConfirm={() => remove.mutate()}
          onCancel={() => setConfirmDelete(false)}
        >
          <p>{t('detail.deleteBody')}</p>
        </ConfirmDialog>
      )}
    </>
  )
}

// Matches the open sheet's top and the handle height in index.css.
const SHEET_OPEN_TOP = 210
const SHEET_HANDLE = 28
const SHEET_SNAP = 40

// Space between neighbouring images while they slide; matches .slide in index.css.
const SLIDE_GAP = 16

function isTap(dx: number, dy: number) {
  return Math.abs(dx) < 10 && Math.abs(dy) < 10
}

function isEditable(target: EventTarget | null) {
  return target instanceof HTMLElement && (target.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName))
}

export function Detail() {
  const { t } = useI18n()
  const qc = useQueryClient()
  const navigate = useNavigate()
  const location = useLocation()
  const params = useParams()
  const postId = Number(params.postId)
  const index = Number(params.index) || 0
  const { tiles, hasMore, loadMore } = useOutletContext<GalleryContext>()
  const [sheetOpen, setSheetOpen] = useState(false)
  const [immersive, setImmersive] = useState(false)
  const start = useRef<{ x: number; y: number } | null>(null)
  const root = useRef<HTMLDivElement>(null)
  const panel = useRef<HTMLElement>(null)
  const drag = useRef<{ y: number; top: number; moved: boolean } | null>(null)
  const [dragTop, setDragTop] = useState<number | null>(null)
  const stage = useRef<HTMLElement>(null)
  const axis = useRef<'x' | 'y' | null>(null)
  const pending = useRef<Tile | null>(null)
  const [offset, setOffset] = useState(0)
  const [animating, setAnimating] = useState(false)

  const { data: post, error } = useQuery({
    queryKey: ['post', postId],
    queryFn: () => api.post(postId),
    refetchInterval: (q) => (q.state.data?.state === 'pending' ? 2000 : false),
  })
  const retry = useMutation({
    mutationFn: () => api.retryPost(postId),
    onSettled: () => (qc.invalidateQueries({ queryKey: ['post', postId] }), qc.invalidateQueries({ queryKey: ['tiles'] })),
  })

  const position = tiles.findIndex((x) => x.post_id === postId && x.index === index)
  const prev = position > 0 ? tiles[position - 1] : null
  const next = position >= 0 && position < tiles.length - 1 ? tiles[position + 1] : null
  const search = location.search

  const close = useCallback(() => navigate(`/${search}`), [navigate, search])
  const go = useCallback(
    (tile: Tile | null) => tile && navigate(`/posts/${tile.post_id}/${tile.index}${search}`, { replace: true }),
    [navigate, search],
  )

  // Slides the track by one stage width, then swaps in the neighbour once the transition ends.
  const slide = useCallback(
    (tile: Tile | null, dir: 1 | -1) => {
      if (!tile || pending.current) return
      const width = stage.current?.clientWidth ?? 0
      if (!width || matchMedia('(prefers-reduced-motion: reduce)').matches) return go(tile)
      pending.current = tile
      setAnimating(true)
      setOffset(-dir * (width + SLIDE_GAP))
    },
    [go],
  )
  const settle = () => {
    const tile = pending.current
    if (tile) go(tile)
    else setAnimating(false)
  }

  // A new tile starts centred and still; the neighbour it slid in from was showing the same picture there.
  const shown = `${postId}/${index}`
  const [lastShown, setLastShown] = useState(shown)
  if (lastShown !== shown) {
    setLastShown(shown)
    setAnimating(false)
    setOffset(0)
  }
  useEffect(() => {
    pending.current = null
  }, [shown])

  useEffect(() => {
    if (position >= tiles.length - 3 && hasMore) loadMore()
  }, [position, tiles.length, hasMore, loadMore])

  useEffect(() => {
    for (const tile of [prev, next]) {
      if (tile) qc.prefetchQuery({ queryKey: ['post', tile.post_id], queryFn: () => api.post(tile.post_id) })
    }
  }, [prev, next, qc])

  useEffect(() => {
    root.current?.focus()
    const overflow = document.body.style.overflow
    document.body.style.overflow = 'hidden'
    return () => {
      document.body.style.overflow = overflow
    }
  }, [])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (isEditable(e.target) || document.querySelector('dialog[open]')) return
      if (e.key === 'ArrowLeft') slide(prev, -1)
      if (e.key === 'ArrowRight') slide(next, 1)
      if (e.key === 'Escape') {
        if (sheetOpen) setSheetOpen(false)
        else close()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [prev, next, slide, close, sheetOpen])

  const media = post?.status?.media ?? []
  const current = media[index]
  // A horizontal drag moves the track with the finger; vertical ones keep the swipe gestures.
  const swipe = {
    onPointerDown: (e: PointerEvent) => {
      if (pending.current) return
      start.current = { x: e.clientX, y: e.clientY }
      axis.current = null
    },
    onPointerMove: (e: PointerEvent<HTMLElement>) => {
      if (!start.current) return
      const dx = e.clientX - start.current.x
      const dy = e.clientY - start.current.y
      if (!axis.current) {
        if (isTap(dx, dy)) return
        axis.current = Math.abs(dx) > Math.abs(dy) ? 'x' : 'y'
        if (axis.current === 'x') e.currentTarget.setPointerCapture(e.pointerId)
      }
      if (axis.current === 'x') setOffset((dx < 0 ? next : prev) ? dx : dx / 3)
    },
    onPointerCancel: () => {
      start.current = null
      if (axis.current === 'x') settleBack()
    },
    onPointerUp: (e: PointerEvent) => {
      if (!start.current) return
      const dx = e.clientX - start.current.x
      const dy = e.clientY - start.current.y
      const dir = swipeDirection(dx, dy)
      start.current = null
      if (axis.current === 'x') {
        const width = stage.current?.clientWidth ?? 0
        const far = Math.abs(dx) > Math.min(80, width / 5)
        if (far && dx < 0 && next) slide(next, 1)
        else if (far && dx > 0 && prev) slide(prev, -1)
        else settleBack()
        return
      }
      if (isTap(dx, dy) && !(e.target as Element).closest('button, a, video')) {
        if (sheetOpen) setSheetOpen(false)
        else setImmersive((on) => !on)
      }
      if (dir === 'up') {
        setSheetOpen(true)
        setImmersive(false)
      }
      if (dir === 'down') {
        if (sheetOpen) setSheetOpen(false)
        else close()
      }
    },
  }

  function settleBack() {
    setAnimating(true)
    setOffset(0)
  }

  // The sheet follows the finger while its handle is dragged, then settles open or closed.
  const handleDrag = {
    onPointerDown: (e: PointerEvent<HTMLButtonElement>) => {
      if (!panel.current) return
      e.currentTarget.setPointerCapture(e.pointerId)
      drag.current = { y: e.clientY, top: panel.current.offsetTop, moved: false }
    },
    onPointerMove: (e: PointerEvent) => {
      const d = drag.current
      if (!d || !root.current) return
      const dy = e.clientY - d.y
      if (!d.moved && Math.abs(dy) < 5) return
      d.moved = true
      setDragTop(Math.min(root.current.clientHeight - SHEET_HANDLE, Math.max(SHEET_OPEN_TOP, d.top + dy)))
    },
    onPointerUp: (e: PointerEvent) => {
      const d = drag.current
      if (!d?.moved) return
      const dy = e.clientY - d.y
      if (dy < -SHEET_SNAP) setSheetOpen(true)
      if (dy > SHEET_SNAP) setSheetOpen(false)
      setDragTop(null)
    },
    onPointerCancel: () => {
      drag.current = null
      setDragTop(null)
    },
    onClick: (e: MouseEvent) => {
      // detail 0 is a keyboard press, which never follows a drag.
      if (e.detail === 0 || !drag.current?.moved) setSheetOpen((o) => !o)
      drag.current = null
    },
  }
  const dragging = dragTop !== null

  return (
    <div
      ref={root}
      tabIndex={-1}
      className={['detail', (sheetOpen || dragging) && 'sheet-open', dragging && 'sheet-dragging', immersive && 'immersive'].filter(Boolean).join(' ')}
      role="dialog"
      aria-modal="true"
      aria-label={t('detail.content')}
    >
      <section ref={stage} className="stage" aria-label={t('detail.media')} {...swipe}>
        <div className="stage-top">
          <button type="button" className="stage-btn" aria-label={t('common.close')} onClick={close}>
            <Icon name="close" size={20} strokeWidth={2} />
          </button>
          {media.length > 1 ? (
            <span className="counter mono" data-testid="counter">
              {index + 1} / {media.length}
            </span>
          ) : (
            <span />
          )}
          {current ? (
            <a className="stage-btn pill" href={current.src} download aria-label={t('detail.download')}>
              <Icon name="download" size={16} strokeWidth={2} />
              <span className="pill-label">{t('detail.download')}</span>
            </a>
          ) : (
            <span />
          )}
        </div>
        {prev && (
          <button type="button" className="stage-nav prev" aria-label={t('detail.prev')} onClick={() => slide(prev, -1)}>
            <Icon name="chevronLeft" size={22} strokeWidth={2} />
          </button>
        )}
        <div
          className={animating ? 'stage-track animating' : 'stage-track'}
          style={{ transform: `translateX(${offset}px)` }}
          onTransitionEnd={(e) => e.target === e.currentTarget && settle()}
        >
          {prev && (
            <div className="slide prev" aria-hidden="true">
              <TilePreview tile={prev} />
            </div>
          )}
          <div className="slide">
            {error ? (
              <p className="stage-message">{errorMessage(t, error)}</p>
            ) : !post ? (
              tiles[position] ? <TilePreview tile={tiles[position]} /> : <span className="spinner" aria-label={t('common.loading')} />
            ) : post.state !== 'done' ? (
              <div className="stage-message">
                <p>{t(post.state === 'pending' ? 'detail.pending' : 'detail.failed')}</p>
                {post.failure && <p className="muted small">{t(`failure.${post.failure}` as 'failure.api_unavailable')}</p>}
                {post.state === 'failed' && (
                  <button type="button" className="btn" disabled={retry.isPending} onClick={() => retry.mutate()}>
                    <Icon name="retry" size={14} strokeWidth={2} /> {t('common.retry')}
                  </button>
                )}
                {retry.error && <p className="form-error">{errorMessage(t, retry.error)}</p>}
              </div>
            ) : post.video ? (
              <YoutubePlayer video={post.video} />
            ) : current ? (
              <Media media={current} />
            ) : (
              <div className="stage-text">{post.status?.text}</div>
            )}
          </div>
          {next && (
            <div className="slide next" aria-hidden="true">
              <TilePreview tile={next} />
            </div>
          )}
        </div>
        {next && (
          <button type="button" className="stage-nav next" aria-label={t('detail.next')} onClick={() => slide(next, 1)}>
            <Icon name="chevronRight" size={22} strokeWidth={2} />
          </button>
        )}
      </section>
      <aside ref={panel} className="panel" aria-label={t('detail.content')} style={dragging ? { top: dragTop } : undefined}>
        <button type="button" className="sheet-handle" aria-label={t('detail.swipeHint')} aria-expanded={sheetOpen} {...handleDrag}>
          <span />
        </button>
        {post && <Panel post={post} />}
      </aside>
    </div>
  )
}
