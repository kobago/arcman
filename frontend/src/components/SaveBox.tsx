import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import { api, type Sidebar } from '../api'
import { folderLabel } from '../folders'
import { useI18n } from '../i18n'
import { Icon } from './Icon'
import { useSaveToasts, type Resolved } from '../saveToasts'

type Props = { variant: 'bar' | 'sheet'; folderId: number | null; destination: string }

export function SaveBox({ variant, folderId, destination }: Props) {
  const { t } = useI18n()
  const qc = useQueryClient()
  const toasts = useSaveToasts()
  const [text, setText] = useState('')

  // Reports to the toast from inside mutationFn, which keeps running when the mobile sheet closes.
  const save = useMutation({
    mutationFn: async (urls: string[]) => {
      const toast = toasts.start(urls, destination)
      try {
        const res = await api.savePosts(urls, folderId)
        const sidebar = qc.getQueryData<Sidebar>(['sidebar'])
        const resolved = await Promise.all(
          res.map(async (r): Promise<Resolved> => {
            if (r.outcome !== 'duplicate' || !r.post_id) return r
            const post = await api.post(r.post_id).catch(() => null)
            return { ...r, folderName: post ? folderLabel(sidebar, post.folder_id) : null }
          }),
        )
        toasts.finish(toast, resolved)
        qc.invalidateQueries({ queryKey: ['tiles'] })
        qc.invalidateQueries({ queryKey: ['authors'] })
      } catch (e) {
        toasts.fail(toast, e)
        throw e
      }
    },
  })

  const submit = () => {
    const urls = text.split(/\s+/).map((s) => s.trim()).filter(Boolean)
    if (!urls.length) return
    save.mutate(urls)
    setText('')
  }

  const lines = text.split('\n').length

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
          <button type="button" className="btn btn-primary" onClick={submit}>
            {t('common.save')}
          </button>
        </div>
      </div>
    )
  }

  return (
    <div className="save-bar">
      <label htmlFor="save-bar" className="visually-hidden">
        {t('save.label')}
      </label>
      <div className="field">
        <Icon name="link" />
        {field}
        <button type="button" className="btn btn-primary btn-sm" onClick={submit}>
          {t('common.save')}
        </button>
      </div>
    </div>
  )
}
