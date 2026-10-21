import { useQuery } from '@tanstack/react-query'
import { useId, useState } from 'react'
import { api, type TagRef } from '../api'
import { normalizeTagKey } from '../format'
import { useI18n } from '../i18n'
import { Icon } from './Icon'

type Props = { value: TagRef[]; onChange: (names: string[]) => void; busy?: boolean }

export function TagInput({ value, onChange, busy }: Props) {
  const { t } = useI18n()
  const id = useId()
  const [text, setText] = useState('')
  const [active, setActive] = useState(0)
  const { data: all = [] } = useQuery({ queryKey: ['tags', 'name'], queryFn: () => api.tags('name') })

  const key = normalizeTagKey(text)
  const taken = new Set(value.map((v) => normalizeTagKey(v.name)))
  const matches = key ? all.filter((x) => normalizeTagKey(x.name).includes(key) && !taken.has(normalizeTagKey(x.name))).slice(0, 6) : []
  const exact = all.some((x) => normalizeTagKey(x.name) === key) || taken.has(key)
  const options = [...matches.map((m) => m.name), ...(key && !exact ? [text.trim().replace(/^#+/, '')] : [])]

  const add = (name: string) => {
    if (!normalizeTagKey(name) || taken.has(normalizeTagKey(name))) return
    onChange([...value.map((v) => v.name), name])
    setText('')
    setActive(0)
  }

  return (
    <div className="tag-input">
      <div className="tag-field">
        {value.map((tag) => (
          <span key={tag.id} className="tag-chip">
            {tag.color && <span className="dot" style={{ background: tag.color }} />}
            {tag.name}
            <button
              type="button"
              aria-label={t('tags.remove', { name: tag.name })}
              disabled={busy}
              onClick={() => onChange(value.filter((v) => v.id !== tag.id).map((v) => v.name))}
            >
              <Icon name="close" size={9} strokeWidth={3} />
            </button>
          </span>
        ))}
        <input
          id={id}
          aria-label={t('tags.add')}
          role="combobox"
          aria-expanded={options.length > 0}
          aria-controls={`${id}-list`}
          aria-autocomplete="list"
          placeholder={value.length ? '' : t('tags.add')}
          value={text}
          onChange={(e) => (setText(e.target.value), setActive(0))}
          onKeyDown={(e) => {
            if (e.nativeEvent.isComposing) return
            if (e.key === 'ArrowDown') {
              e.preventDefault()
              setActive((a) => Math.min(a + 1, options.length - 1))
            }
            if (e.key === 'ArrowUp') {
              e.preventDefault()
              setActive((a) => Math.max(a - 1, 0))
            }
            if (e.key === 'Enter' && options.length) {
              e.preventDefault()
              add(options[active] ?? text)
            }
            if (e.key === 'Backspace' && !text && value.length) onChange(value.slice(0, -1).map((v) => v.name))
          }}
        />
      </div>
      {options.length > 0 && (
        <ul id={`${id}-list`} role="listbox" aria-label={t('tags.suggestions')} className="tag-options">
          {options.map((name, i) => {
            const isNew = i >= matches.length
            const count = matches[i] ? all.find((x) => x.name === name)?.count : undefined
            return (
              <li
                key={`${name}-${i}`}
                role="option"
                aria-selected={i === active}
                className={i === active ? 'active' : undefined}
                onPointerDown={(e) => (e.preventDefault(), add(name))}
              >
                {isNew ? <span className="accent-text">{t('tags.create', { name })}</span> : name}
                {count !== undefined && <span className="mono muted small">{count}</span>}
              </li>
            )
          })}
        </ul>
      )}
    </div>
  )
}
