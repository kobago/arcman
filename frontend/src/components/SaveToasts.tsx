import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { Link, useLocation } from 'react-router'
import { api, type PostProgress, type SaveResult } from '../api'
import { errorMessage, useI18n } from '../i18n'
import { SaveToastsContext, type Resolved, type SaveToastsApi } from '../saveToasts'
import { Icon } from './Icon'

type Toast = { id: number; urls: string[]; destination: string; results?: Resolved[]; error?: unknown }

const DISMISS_MS = 5000

export function SaveToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([])
  const nextId = useRef(0)

  const patch = useCallback((id: number, change: Partial<Toast>) => {
    setToasts((ts) => ts.map((x) => (x.id === id ? { ...x, ...change } : x)))
  }, [])
  const close = useCallback((id: number) => setToasts((ts) => ts.filter((x) => x.id !== id)), [])

  const value = useMemo<SaveToastsApi>(
    () => ({
      start: (urls, destination) => {
        const id = ++nextId.current
        setToasts((ts) => [...ts, { id, urls, destination }])
        return id
      },
      finish: (id, results) => patch(id, { results }),
      fail: (id, error) => patch(id, { error }),
    }),
    [patch],
  )

  return (
    <SaveToastsContext.Provider value={value}>
      {children}
      <div className="toasts">
        {toasts.map((x) => (
          <SaveToast key={x.id} toast={x} close={close} />
        ))}
      </div>
    </SaveToastsContext.Provider>
  )
}

type Row = { url: string; outcome: SaveResult['outcome'] | 'saving' | 'error'; post: Resolved | null; progress?: PostProgress }

function SaveToast({ toast, close }: { toast: Toast; close: (id: number) => void }) {
  const { t } = useI18n()
  const onClose = useCallback(() => close(toast.id), [close, toast.id])
  const qc = useQueryClient()
  const location = useLocation()

  const created = toast.results?.filter((r) => r.outcome === 'created' && r.post_id).map((r) => r.post_id!) ?? []
  const progress = useQuery({
    queryKey: ['progress', created.join(',')],
    queryFn: () => api.progress(created),
    enabled: created.length > 0,
    staleTime: 0,
    refetchInterval: (q) => (q.state.data && q.state.data.every((p) => p.state !== 'pending') ? false : 700),
  })
  const byId = new Map(progress.data?.map((p) => [p.post_id, p]))

  const rows: Row[] = toast.results
    ? toast.results.map((r) => ({ url: r.url, outcome: r.outcome, post: r, progress: r.post_id ? byId.get(r.post_id) : undefined }))
    : toast.urls.map((url) => ({ url, outcome: toast.error ? 'error' : 'saving', post: null }))

  const downloading = created.filter((id) => byId.get(id)?.state !== 'done' && byId.get(id)?.state !== 'failed')
  const downloaded = created.filter((id) => byId.get(id)?.state === 'done').length
  const failedDownloads = created.length - downloading.length - downloaded
  const settled = toast.error !== undefined || (toast.results !== undefined && downloading.length === 0)
  const clean =
    settled &&
    toast.error === undefined &&
    failedDownloads === 0 &&
    toast.results!.every((r) => r.outcome === 'created' || r.outcome === 'duplicate')

  useEffect(() => {
    if (!settled || created.length === 0) return
    qc.invalidateQueries({ queryKey: ['tiles'] })
  }, [settled, created.length, qc])

  useEffect(() => {
    if (!clean) return
    const timer = setTimeout(onClose, DISMISS_MS)
    return () => clearTimeout(timer)
  }, [clean, onClose])

  const head = !toast.results
    ? toast.error
      ? errorMessage(t, toast.error)
      : t('save.saving', { count: toast.urls.length })
    : downloading.length
      ? t('save.downloading', { done: downloaded, total: created.length })
      : t('save.saved', { count: created.length })

  const rowState = (r: Row): { cls: string; mark: string; msg: ReactNode; value?: number | null } => {
    switch (r.outcome) {
      case 'saving':
        return { cls: 'saving', mark: '', msg: t('save.fetching'), value: null }
      case 'created': {
        const p = r.progress
        if (p?.state === 'done') return { cls: 'created', mark: '✓', msg: t('save.downloaded'), value: 1 }
        if (p?.state === 'failed') return { cls: 'failed', mark: '!', msg: t('failure.media_download') }
        if (p?.progress == null) return { cls: 'saving', mark: '', msg: t('save.queued'), value: 0 }
        return { cls: 'saving', mark: '', msg: `${t('save.downloadingOne')} ${Math.round(p.progress * 100)}%`, value: p.progress }
      }
      case 'duplicate': {
        const post = r.post!
        return {
          cls: 'duplicate',
          mark: '=',
          msg: (
            <>
              {post.folderName ? t('save.duplicateIn', { folder: post.folderName }) : t('save.duplicate')}{' '}
              {post.post_id && (
                <Link to={`/posts/${post.post_id}/0${location.search}`} onClick={onClose}>
                  {t('save.openPost')}
                </Link>
              )}
            </>
          ),
        }
      }
      case 'error':
        return { cls: 'failed', mark: '!', msg: '' }
      case 'invalid_url':
        return { cls: 'invalid_url', mark: '!', msg: t('save.invalid_url') }
      case 'unavailable':
        return { cls: 'unavailable', mark: '!', msg: t('save.unavailable') }
    }
  }

  return (
    <div className={toast.error ? 'toast save-toast error' : 'toast save-toast'} role="status">
      <div className="save-results-head">
        <strong>{head}</strong>
        <button type="button" className="icon-btn small" aria-label={t('common.close')} onClick={onClose}>
          <Icon name="close" size={14} />
        </button>
      </div>
      <div className="save-destination">
        {t('save.destination')}
        <span className="chip-static">
          <Icon name="folder" size={14} />
          {toast.destination}
        </span>
      </div>
      <ul>
        {rows.map((r, i) => {
          const s = rowState(r)
          return (
            <li key={i} className={`save-result ${s.cls}`}>
              <span className="mark" aria-hidden="true">
                {s.mark}
              </span>
              <span className="url mono ellipsis">{r.url.replace(/^https?:\/\//, '')}</span>
              <span />
              <span className="msg">{s.msg}</span>
              {s.value !== undefined && (
                <>
                  <span />
                  <span
                    className={s.value === null ? 'progress indeterminate' : 'progress'}
                    role="progressbar"
                    aria-label={r.url}
                    aria-valuemin={0}
                    aria-valuemax={100}
                    aria-valuenow={s.value === null ? undefined : Math.round(s.value * 100)}
                  >
                    <span style={s.value === null ? undefined : { width: `${s.value * 100}%` }} />
                  </span>
                </>
              )}
            </li>
          )
        })}
      </ul>
    </div>
  )
}
