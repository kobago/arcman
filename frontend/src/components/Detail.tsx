import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useCallback, useEffect, useRef, useState, type PointerEvent } from 'react'
import { useLocation, useNavigate, useOutletContext, useParams } from 'react-router'
import { api, type MediaView, type PostDetail, type StatusView, type Tile } from '../api'
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
  const start = useRef<{ x: number; y: number } | null>(null)
  const root = useRef<HTMLDivElement>(null)

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

  useEffect(() => {
    if (position >= tiles.length - 3 && hasMore) loadMore()
  }, [position, tiles.length, hasMore, loadMore])

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
      if (e.key === 'ArrowLeft') go(prev)
      if (e.key === 'ArrowRight') go(next)
      if (e.key === 'Escape') {
        if (sheetOpen) setSheetOpen(false)
        else close()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [prev, next, go, close, sheetOpen])

  const media = post?.status?.media ?? []
  const current = media[index]
  const swipe = {
    onPointerDown: (e: PointerEvent) => {
      start.current = { x: e.clientX, y: e.clientY }
    },
    onPointerCancel: () => {
      start.current = null
    },
    onPointerUp: (e: PointerEvent) => {
      if (!start.current) return
      const dir = swipeDirection(e.clientX - start.current.x, e.clientY - start.current.y)
      start.current = null
      if (dir === 'left') go(next)
      if (dir === 'right') go(prev)
      if (dir === 'up') setSheetOpen(true)
      if (dir === 'down') {
        if (sheetOpen) setSheetOpen(false)
        else close()
      }
    },
  }

  return (
    <div ref={root} tabIndex={-1} className={sheetOpen ? 'detail sheet-open' : 'detail'} role="dialog" aria-modal="true" aria-label={t('detail.content')}>
      <section className="stage" aria-label={t('detail.media')} {...swipe}>
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
          <button type="button" className="stage-nav prev" aria-label={t('detail.prev')} onClick={() => go(prev)}>
            <Icon name="chevronLeft" size={22} strokeWidth={2} />
          </button>
        )}
        {error ? (
          <p className="stage-message">{errorMessage(t, error)}</p>
        ) : !post ? (
          <span className="spinner" aria-label={t('common.loading')} />
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
        ) : current ? (
          <Media media={current} />
        ) : (
          <div className="stage-text">{post.status?.text}</div>
        )}
        {next && (
          <button type="button" className="stage-nav next" aria-label={t('detail.next')} onClick={() => go(next)}>
            <Icon name="chevronRight" size={22} strokeWidth={2} />
          </button>
        )}
      </section>
      <aside className="panel" aria-label={t('detail.content')}>
        <button type="button" className="sheet-handle" aria-label={t('detail.swipeHint')} aria-expanded={sheetOpen} onClick={() => setSheetOpen((o) => !o)} {...swipe}>
          <span />
        </button>
        {post && <Panel post={post} />}
        {!sheetOpen && <span className="swipe-hint muted small">{t('detail.swipeHint')}</span>}
      </aside>
    </div>
  )
}
