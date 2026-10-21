import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import { Link } from 'react-router'
import { api, ApiError, type Tag } from '../api'
import { ConfirmDialog } from '../components/ConfirmDialog'
import { Icon } from '../components/Icon'
import { normalizeTagKey } from '../format'
import { useDismiss } from '../hooks'
import { errorMessage, useI18n } from '../i18n'

const PALETTE = ['#e06c6c', '#e6a04e', '#d9c55a', '#7cc49a', '#5fb3c7', '#7a9cf0', '#b08ae6', '#a3a9b0']

function ColorPicker({ tag, onPick }: { tag: Tag; onPick: (c: string | null) => void }) {
  const { t } = useI18n()
  const [open, setOpen] = useState(false)
  const ref = useDismiss(open, () => setOpen(false))
  return (
    <div className="dropdown" ref={ref}>
      <button
        type="button"
        className="swatch"
        style={{ background: tag.color ?? 'transparent' }}
        aria-label={t('tags.changeColor', { name: tag.name })}
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
      />
      {open && (
        <div className="popover palette" role="group" aria-label={t('tags.pickColor')}>
          {PALETTE.map((c) => (
            <button
              key={c}
              type="button"
              className={c === tag.color ? 'swatch selected' : 'swatch'}
              style={{ background: c }}
              aria-label={c}
              aria-pressed={c === tag.color}
              onClick={() => (setOpen(false), onPick(c))}
            />
          ))}
          <button type="button" className="btn btn-ghost btn-sm" onClick={() => (setOpen(false), onPick(null))}>
            {t('tags.noColor')}
          </button>
        </div>
      )}
    </div>
  )
}

export function TagsPage() {
  const { t } = useI18n()
  const qc = useQueryClient()
  const [sort, setSort] = useState<'name' | 'count'>('count')
  const [filter, setFilter] = useState('')
  const [editing, setEditing] = useState<{ id: number; name: string } | null>(null)
  const [deleting, setDeleting] = useState<Tag | null>(null)
  const { data: tags = [] } = useQuery({ queryKey: ['tags', sort], queryFn: () => api.tags(sort) })

  const done = () => qc.invalidateQueries({ queryKey: ['tags'] })
  const update = useMutation({
    mutationFn: ({ id, patch }: { id: number; patch: { name?: string; color?: string | null } }) => api.updateTag(id, patch),
    onSuccess: (_, v) => {
      if (v.patch.name !== undefined) setEditing(null)
    },
    onSettled: done,
  })
  const remove = useMutation({
    mutationFn: (id: number) => api.deleteTag(id),
    onSettled: () => (setDeleting(null), done(), qc.invalidateQueries({ queryKey: ['tiles'] })),
  })

  const key = normalizeTagKey(filter)
  const visible = tags.filter((x) => !key || normalizeTagKey(x.name).includes(key))
  const renameError =
    update.error instanceof ApiError && update.error.code === 'tag_exists' && editing
      ? t('tags.exists', { name: editing.name })
      : update.error
        ? errorMessage(t, update.error)
        : null

  return (
    <div className="page">
      <div className="page-inner">
        <Link to="/" className="back-link">
          <Icon name="chevronLeft" size={14} strokeWidth={2} /> {t('common.backToGallery')}
        </Link>
        <div className="page-head">
          <h1>{t('tags.title')}</h1>
          <span className="muted">{t('tags.count', { count: tags.length })}</span>
        </div>
        <div className="toolbar">
          <div className="search-field open">
            <Icon name="search" />
            <input type="search" aria-label={t('tags.filter')} placeholder={t('tags.filter')} value={filter} onChange={(e) => setFilter(e.target.value)} />
          </div>
          <div className="segmented" role="group" aria-label={t('tags.sort')}>
            {(['name', 'count'] as const).map((s) => (
              <button key={s} type="button" aria-pressed={sort === s} onClick={() => setSort(s)}>
                {t(s === 'name' ? 'tags.byName' : 'tags.byCount')}
              </button>
            ))}
          </div>
        </div>

        {tags.length === 0 ? (
          <p className="empty">{t('tags.none')}</p>
        ) : (
          <div className="table" role="table" aria-label={t('tags.title')}>
            <div className="table-row head" role="row">
              <span role="columnheader">{t('tags.colColor')}</span>
              <span role="columnheader">{t('tags.colName')}</span>
              <span role="columnheader" className="num">
                {t('tags.colCount')}
              </span>
              <span role="columnheader" className="num">
                {t('tags.colActions')}
              </span>
            </div>
            {visible.map((tag) => (
              <div className="table-row" role="row" key={tag.id}>
                <span role="cell">
                  <ColorPicker tag={tag} onPick={(color) => update.mutate({ id: tag.id, patch: { color } })} />
                </span>
                <span role="cell">
                  {editing?.id === tag.id ? (
                    <form
                      className="rename-form"
                      onSubmit={(e) => {
                        e.preventDefault()
                        update.mutate({ id: tag.id, patch: { name: editing.name } })
                      }}
                    >
                      <input className="input" aria-label={t('tags.newName')} autoFocus value={editing.name} onChange={(e) => setEditing({ id: tag.id, name: e.target.value })} />
                      <button type="submit" className="btn btn-primary btn-sm">
                        {t('common.save')}
                      </button>
                      <button type="button" className="btn btn-ghost btn-sm" onClick={() => (setEditing(null), update.reset())}>
                        {t('common.cancel')}
                      </button>
                      {renameError && (
                        <span className="form-error" role="alert">
                          {renameError}
                        </span>
                      )}
                    </form>
                  ) : (
                    tag.name
                  )}
                </span>
                <span role="cell" className="num mono">
                  {tag.count}
                </span>
                <span role="cell" className="actions">
                  <Link to={`/?tags=${tag.id}`}>{t('tags.viewPosts')}</Link>
                  <button type="button" className="icon-btn small" aria-label={t('tags.renameTag', { name: tag.name })} onClick={() => (update.reset(), setEditing({ id: tag.id, name: tag.name }))}>
                    <Icon name="pencil" size={15} />
                  </button>
                  <button type="button" className="icon-btn small" aria-label={t('tags.deleteTag', { name: tag.name })} onClick={() => setDeleting(tag)}>
                    <Icon name="trash" size={15} />
                  </button>
                </span>
              </div>
            ))}
          </div>
        )}
      </div>
      {deleting && (
        <ConfirmDialog
          title={t('tags.deleteTitle', { name: deleting.name })}
          confirmLabel={t('common.delete')}
          danger
          busy={remove.isPending}
          onConfirm={() => remove.mutate(deleting.id)}
          onCancel={() => setDeleting(null)}
        >
          <p>{t('tags.deleteBody', { count: deleting.count })}</p>
        </ConfirmDialog>
      )}
    </div>
  )
}
