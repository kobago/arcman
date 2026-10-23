import { useQuery } from '@tanstack/react-query'
import { useState, type ReactNode } from 'react'
import { api } from '../api'
import type { Filters, MediaFilter } from '../filters'
import { normalizeTagKey } from '../format'
import { useDismiss } from '../hooks'
import { useI18n } from '../i18n'
import { Icon } from './Icon'

type Props = { filters: Filters; onChange: (f: Filters) => void; title?: ReactNode; trailing?: ReactNode }
type Open = 'tags' | 'period' | 'media' | 'author' | null

function Dropdown({ label, open, onToggle, children }: { label: string; open: boolean; onToggle: () => void; children: ReactNode }) {
  const ref = useDismiss(open, onToggle)
  return (
    <div className="dropdown" ref={ref}>
      <button type="button" className={open ? 'chip-btn open' : 'chip-btn'} aria-expanded={open} onClick={onToggle}>
        {label}
        <Icon name="chevronDown" size={12} strokeWidth={2.2} />
      </button>
      {open && <div className="popover dropdown-panel">{children}</div>}
    </div>
  )
}

function ActiveChip({ label, onRemove }: { label: string; onRemove: () => void }) {
  const { t } = useI18n()
  return (
    <span className="chip-active">
      {label}
      <button type="button" aria-label={t('filter.remove', { name: label })} onClick={onRemove}>
        <Icon name="close" size={10} strokeWidth={3} />
      </button>
    </span>
  )
}

export function FilterBar({ filters, onChange, title, trailing }: Props) {
  const { t } = useI18n()
  const [open, setOpen] = useState<Open>(null)
  const [tagQuery, setTagQuery] = useState('')
  const [from, setFrom] = useState(filters.from)
  const [to, setTo] = useState(filters.to)
  const tags = useQuery({ queryKey: ['tags', 'name'], queryFn: () => api.tags('name') })
  const authors = useQuery({ queryKey: ['authors'], queryFn: api.authors, enabled: open === 'author' || !!filters.author })
  const toggle = (o: Open) => () => setOpen((cur) => (cur === o ? null : o))
  const set = (patch: Partial<Filters>) => onChange({ ...filters, ...patch })

  const tagName = (id: number) => tags.data?.find((x) => x.id === id)?.name ?? `#${id}`
  const mediaLabel: Record<Exclude<MediaFilter, ''>, string> = {
    photo: t('filter.media.photo'),
    video: t('filter.media.video'),
    text: t('filter.media.text'),
  }
  const key = normalizeTagKey(tagQuery)
  const visibleTags = (tags.data ?? []).filter((x) => !key || normalizeTagKey(x.name).includes(key))

  return (
    <div className="filter-bar">
      {title}
      {filters.tags.map((id) => (
        <ActiveChip key={id} label={tagName(id)} onRemove={() => set({ tags: filters.tags.filter((x) => x !== id) })} />
      ))}
      {(filters.from || filters.to) && (
        <ActiveChip label={`${filters.from || '…'} – ${filters.to || '…'}`} onRemove={() => (set({ from: '', to: '' }), setFrom(''), setTo(''))} />
      )}
      {filters.media && <ActiveChip label={mediaLabel[filters.media]} onRemove={() => set({ media: '' })} />}
      {filters.author && <ActiveChip label={`@${filters.author}`} onRemove={() => set({ author: '' })} />}

      <Dropdown label={t('filter.tags')} open={open === 'tags'} onToggle={toggle('tags')}>
        <input className="input" type="search" aria-label={t('filter.findTag')} placeholder={t('filter.findTag')} value={tagQuery} onChange={(e) => setTagQuery(e.target.value)} />
        <div className="option-list">
          {visibleTags.length === 0 && <p className="muted small">{t('filter.noTags')}</p>}
          {visibleTags.map((tag) => (
            <label key={tag.id} className="option">
              <input
                type="checkbox"
                checked={filters.tags.includes(tag.id)}
                onChange={(e) => set({ tags: e.target.checked ? [...filters.tags, tag.id] : filters.tags.filter((x) => x !== tag.id) })}
              />
              {tag.color && <span className="dot" style={{ background: tag.color }} />}
              <span className="ellipsis">{tag.name}</span>
              <span className="mono muted small">{tag.count}</span>
            </label>
          ))}
        </div>
      </Dropdown>

      <Dropdown label={t('filter.period')} open={open === 'period'} onToggle={toggle('period')}>
        <form
          className="period-form"
          onSubmit={(e) => {
            e.preventDefault()
            set({ from, to })
            setOpen(null)
          }}
        >
          <label>
            {t('filter.from')}
            <input className="input" type="date" value={from} max={to || undefined} onChange={(e) => setFrom(e.target.value)} />
          </label>
          <label>
            {t('filter.to')}
            <input className="input" type="date" value={to} min={from || undefined} onChange={(e) => setTo(e.target.value)} />
          </label>
          <div className="row-end">
            <button type="button" className="btn btn-ghost" onClick={() => (setFrom(''), setTo(''))}>
              {t('filter.clear')}
            </button>
            <button type="submit" className="btn btn-primary">
              {t('filter.apply')}
            </button>
          </div>
        </form>
      </Dropdown>

      <Dropdown label={t('filter.media')} open={open === 'media'} onToggle={toggle('media')}>
        <div className="option-list" role="radiogroup" aria-label={t('filter.media')}>
          {(['', 'photo', 'video', 'text'] as MediaFilter[]).map((m) => (
            <label key={m || 'any'} className="option">
              <input type="radio" name="media" checked={filters.media === m} onChange={() => (set({ media: m }), setOpen(null))} />
              {m ? mediaLabel[m] : t('filter.media.any')}
            </label>
          ))}
        </div>
      </Dropdown>

      <Dropdown label={t('filter.author')} open={open === 'author'} onToggle={toggle('author')}>
        <div className="option-list" role="radiogroup" aria-label={t('filter.author')}>
          <label className="option">
            <input type="radio" name="author" checked={!filters.author} onChange={() => (set({ author: '' }), setOpen(null))} />
            {t('filter.author.any')}
          </label>
          {(authors.data ?? []).map((a) => (
            <label key={a.screen_name} className="option">
              <input type="radio" name="author" checked={filters.author.toLowerCase() === a.screen_name.toLowerCase()} onChange={() => (set({ author: a.screen_name }), setOpen(null))} />
              <span className="ellipsis">
                {a.name} <span className="muted">@{a.screen_name}</span>
              </span>
              <span className="mono muted small">{a.count}</span>
            </label>
          ))}
        </div>
      </Dropdown>

      <span className="spacer" />
      <button type="button" className="sort-btn" onClick={() => set({ order: filters.order === 'desc' ? 'asc' : 'desc' })}>
        <Icon name="sort" size={14} strokeWidth={2} />
        {t(filters.order === 'desc' ? 'sort.desc' : 'sort.asc')}
      </button>
      {trailing}
    </div>
  )
}
