import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import { Link, useLocation } from 'react-router'
import { api, type SaveResult, type Sidebar } from '../api'
import { folderLabel } from '../folders'
import { useDismiss } from '../hooks'
import { errorMessage, useI18n } from '../i18n'
import { Icon } from './Icon'

type Resolved = SaveResult & { folderName?: string | null }


type Props = { variant: 'bar' | 'sheet'; folderId: number | null; destination: string }

export function SaveBox({ variant, folderId, destination }: Props) {
  const { t } = useI18n()
  const qc = useQueryClient()
  const location = useLocation()
  const [text, setText] = useState('')
  const [results, setResults] = useState<Resolved[] | null>(null)
  const panelRef = useDismiss(variant === 'bar' && results !== null, () => setResults(null))

  const save = useMutation({
    mutationFn: async (urls: string[]) => {
      const res = await api.savePosts(urls, folderId)
      const sidebar = qc.getQueryData<Sidebar>(['sidebar'])
      return Promise.all(
        res.map(async (r): Promise<Resolved> => {
          if (r.outcome !== 'duplicate' || !r.post_id) return r
          const post = await api.post(r.post_id).catch(() => null)
          return { ...r, folderName: post ? folderLabel(sidebar, post.folder_id) : null }
        }),
      )
    },
    onSuccess: (res) => {
      setResults(res)
      setText('')
      qc.invalidateQueries({ queryKey: ['tiles'] })
      qc.invalidateQueries({ queryKey: ['authors'] })
    },
  })

  const submit = () => {
    const urls = text.split(/\s+/).map((s) => s.trim()).filter(Boolean)
    if (urls.length && !save.isPending) save.mutate(urls)
  }

  const lines = text.split('\n').length
  const saved = results?.filter((r) => r.outcome === 'created').length ?? 0

  const message = (r: Resolved) => {
    switch (r.outcome) {
      case 'created':
        return t('save.created')
      case 'duplicate':
        return r.folderName ? t('save.duplicateIn', { folder: r.folderName }) : t('save.duplicate')
      case 'invalid_url':
        return t('save.invalid_url')
      case 'unavailable':
        return t('save.unavailable')
    }
  }

  const resultList = results && (
    <div className="save-results" role="status">
      <div className="save-results-head">
        <strong>{t('save.saved', { count: saved })}</strong>
        <button type="button" className="icon-btn small" aria-label={t('common.close')} onClick={() => setResults(null)}>
          <Icon name="close" size={14} />
        </button>
      </div>
      <ul>
        {results.map((r, i) => (
          <li key={i} className={`save-result ${r.outcome}`}>
            <span className="mark" aria-hidden="true">
              {r.outcome === 'created' ? '✓' : r.outcome === 'duplicate' ? '=' : '!'}
            </span>
            <span className="url mono ellipsis">{r.url.replace(/^https?:\/\//, '')}</span>
            <span />
            <span className="msg">
              {message(r)}
              {r.outcome === 'duplicate' && r.post_id && (
                <>
                  {' '}
                  <Link to={`/posts/${r.post_id}/0${location.search}`} onClick={() => setResults(null)}>
                    {t('save.openPost')}
                  </Link>
                </>
              )}
            </span>
          </li>
        ))}
      </ul>
    </div>
  )

  const field = (
    <textarea
      id={`save-${variant}`}
      className="save-input"
      rows={variant === 'sheet' ? 4 : Math.min(lines, 6)}
      placeholder={t('save.placeholder')}
      value={text}
      onChange={(e) => setText(e.target.value)}
      onKeyDown={(e) => {
        if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) {
          e.preventDefault()
          submit()
        }
      }}
    />
  )

  const destinationRow = (
    <div className="save-destination">
      {t('save.destination')}
      <span className="chip-static">
        <Icon name="folder" size={14} />
        {destination}
      </span>
      {folderId !== null && <span className="muted small">{t('save.currentFolder')}</span>}
    </div>
  )

  if (variant === 'sheet') {
    return (
      <div className="save-sheet">
        <label htmlFor="save-sheet" className="field-label">
          {t('save.label')}
        </label>
        {field}
        <div className="save-sheet-row">
          {destinationRow}
          <button type="button" className="btn btn-primary" onClick={submit} disabled={save.isPending}>
            {t('common.save')}
          </button>
        </div>
        {save.isError && <p className="form-error" role="alert">{errorMessage(t, save.error)}</p>}
        {resultList}
      </div>
    )
  }

  return (
    <div className="save-bar" ref={panelRef}>
      <label htmlFor="save-bar" className="visually-hidden">
        {t('save.label')}
      </label>
      <div className="field">
        <Icon name="link" />
        {field}
        <button type="button" className="btn btn-primary btn-sm" onClick={submit} disabled={save.isPending}>
          {t('common.save')}
        </button>
      </div>
      {(results || save.isError) && (
        <div className="popover save-popover">
          {destinationRow}
          {save.isError && <p className="form-error" role="alert">{errorMessage(t, save.error)}</p>}
          {resultList}
        </div>
      )}
    </div>
  )
}
