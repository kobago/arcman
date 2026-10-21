import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useState, type DragEvent, type ReactNode } from 'react'
import { Link, useNavigate } from 'react-router'
import { api, type Folder } from '../api'
import { useDismiss } from '../hooks'
import { errorMessage, useI18n } from '../i18n'
import { ConfirmDialog } from './ConfirmDialog'
import { Icon } from './Icon'
import { Logo } from './Logo'

type Group = { id: number | null; name: string; folders: Folder[] }
type Editing =
  | { kind: 'newFolder'; categoryId: number | null }
  | { kind: 'newCategory' }
  | { kind: 'renameFolder'; id: number }
  | { kind: 'renameCategory'; id: number }
type Confirm = { kind: 'folder'; folder: Folder; count: number } | { kind: 'category'; id: number; name: string }
type Drag = { type: 'folder' | 'category'; id: number }

const COLLAPSED_KEY = 'arcman-collapsed'

function readCollapsed(): string[] {
  try {
    return JSON.parse(localStorage.getItem(COLLAPSED_KEY) ?? '[]')
  } catch {
    return []
  }
}


function InlineInput({ label, initial = '', onSubmit, onCancel }: { label: string; initial?: string; onSubmit: (v: string) => void; onCancel: () => void }) {
  const [value, setValue] = useState(initial)
  return (
    <input
      className="inline-input"
      aria-label={label}
      autoFocus
      value={value}
      onChange={(e) => setValue(e.target.value)}
      onBlur={onCancel}
      onKeyDown={(e) => {
        if (e.key === 'Escape') onCancel()
        if (e.key === 'Enter' && !e.nativeEvent.isComposing) {
          e.preventDefault()
          if (value.trim()) onSubmit(value.trim())
          else onCancel()
        }
      }}
    />
  )
}

function ItemMenu({ label, onRename, onDelete }: { label: string; onRename: () => void; onDelete: () => void }) {
  const { t } = useI18n()
  const [open, setOpen] = useState(false)
  const ref = useDismiss(open, () => setOpen(false))
  return (
    <div className="item-menu" ref={ref}>
      <button type="button" className="icon-btn small" aria-label={label} aria-expanded={open} onClick={() => setOpen((o) => !o)}>
        <Icon name="more" size={14} />
      </button>
      {open && (
        <div className="menu" role="menu">
          <button type="button" role="menuitem" onClick={() => (setOpen(false), onRename())}>
            <Icon name="pencil" size={14} /> {t('common.rename')}
          </button>
          <button type="button" role="menuitem" className="danger" onClick={() => (setOpen(false), onDelete())}>
            <Icon name="trash" size={14} /> {t('common.delete')}
          </button>
        </div>
      )}
    </div>
  )
}

type Props = {
  current: string
  linkFor: (folder: string) => string
  onNavigate?: () => void
  header?: ReactNode
}

export function Sidebar({ current, linkFor, onNavigate, header }: Props) {
  const { t } = useI18n()
  const qc = useQueryClient()
  const navigate = useNavigate()
  const { data } = useQuery({ queryKey: ['sidebar'], queryFn: api.sidebar })
  const [collapsed, setCollapsed] = useState<string[]>(readCollapsed)
  const [editing, setEditing] = useState<Editing | null>(null)
  const [confirm, setConfirm] = useState<Confirm | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [drag, setDrag] = useState<Drag | null>(null)
  const [dropKey, setDropKey] = useState<string | null>(null)

  const run = useMutation({
    mutationFn: (fn: () => Promise<unknown>) => fn(),
    onSuccess: () => setError(null),
    onError: (e) => setError(errorMessage(t, e)),
    onSettled: () => qc.invalidateQueries({ queryKey: ['sidebar'] }),
  })

  const groups: Group[] = [
    ...(data?.categories ?? []),
    { id: null, name: t('nav.uncategorized'), folders: data?.uncategorized ?? [] },
  ]

  const toggle = (key: string) => {
    const next = collapsed.includes(key) ? collapsed.filter((k) => k !== key) : [...collapsed, key]
    setCollapsed(next)
    try {
      localStorage.setItem(COLLAPSED_KEY, JSON.stringify(next))
    } catch {
      // Collapsing still works for this visit.
    }
  }

  const askDeleteFolder = async (folder: Folder) => {
    try {
      const { post_count } = await api.folder(folder.id)
      setConfirm({ kind: 'folder', folder, count: post_count })
    } catch (e) {
      setError(errorMessage(t, e))
    }
  }

  const confirmDelete = () => {
    if (!confirm) return
    const c = confirm
    run.mutate(
      async () => {
        if (c.kind === 'folder') {
          await api.deleteFolder(c.folder.id)
          await qc.invalidateQueries({ queryKey: ['tiles'] })
          if (current === String(c.folder.id)) navigate(linkFor('all'))
        } else {
          await api.deleteCategory(c.id)
        }
      },
      { onSettled: () => setConfirm(null) },
    )
  }

  const dropOnFolder = (target: Folder) => {
    if (drag?.type !== 'folder' || drag.id === target.id) return
    const group = groups.find((g) => g.id === target.category_id)
    if (!group) return
    const ids = group.folders.map((f) => f.id).filter((id) => id !== drag.id)
    ids.splice(ids.indexOf(target.id), 0, drag.id)
    run.mutate(() => api.orderFolders(target.category_id, ids))
  }

  const dropOnGroup = (group: Group) => {
    if (drag?.type === 'folder') {
      const ids = [...group.folders.map((f) => f.id).filter((id) => id !== drag.id), drag.id]
      run.mutate(() => api.orderFolders(group.id, ids))
    } else if (drag?.type === 'category' && group.id !== null && drag.id !== group.id) {
      const ids = (data?.categories ?? []).map((c) => c.id).filter((id) => id !== drag.id)
      ids.splice(ids.indexOf(group.id), 0, drag.id)
      run.mutate(() => api.orderCategories(ids))
    }
  }

  const dropProps = (key: string, accepts: (d: Drag) => boolean, onDrop: () => void) => ({
    onDragOver: (e: DragEvent) => {
      if (drag && accepts(drag)) {
        e.preventDefault()
        setDropKey(key)
      }
    },
    onDragLeave: () => setDropKey((k) => (k === key ? null : k)),
    onDrop: (e: DragEvent) => {
      e.preventDefault()
      setDropKey(null)
      onDrop()
      setDrag(null)
    },
  })

  const dragProps = (d: Drag) => ({
    draggable: true,
    onDragStart: (e: DragEvent) => {
      e.dataTransfer.effectAllowed = 'move'
      e.dataTransfer.setData('text/plain', `${d.type}:${d.id}`)
      setDrag(d)
    },
    onDragEnd: () => (setDrag(null), setDropKey(null)),
  })

  const navItem = (folder: string, label: ReactNode, icon: 'grid' | 'list') => (
    <Link to={linkFor(folder)} onClick={onNavigate} className={current === folder ? 'nav-item active' : 'nav-item'} aria-current={current === folder ? 'page' : undefined}>
      <Icon name={icon} />
      {label}
    </Link>
  )

  return (
    <nav className="sidebar" aria-label={t('nav.folders')}>
      <div className="sidebar-head">
        <Logo />
        {header}
      </div>
      <div className="sidebar-scroll">
        {navItem('all', t('nav.all'), 'grid')}
        {navItem('unsorted', t('nav.unsorted'), 'list')}

        {groups.map((group) => {
          const key = group.id === null ? 'none' : String(group.id)
          const isCollapsed = collapsed.includes(key)
          const isCategory = group.id !== null
          return (
            <section key={key} className="nav-group">
              <div
                className={dropKey === `group:${key}` ? 'group-head drop' : 'group-head'}
                {...(isCategory ? dragProps({ type: 'category', id: group.id! }) : {})}
                {...dropProps(`group:${key}`, (d) => d.type === 'folder' || isCategory, () => dropOnGroup(group))}
              >
                {editing?.kind === 'renameCategory' && editing.id === group.id ? (
                  <InlineInput
                    label={t('common.rename')}
                    initial={group.name}
                    onCancel={() => setEditing(null)}
                    onSubmit={(name) => (setEditing(null), run.mutate(() => api.renameCategory(group.id!, name)))}
                  />
                ) : (
                  <button
                    type="button"
                    className="group-toggle"
                    aria-expanded={!isCollapsed}
                    aria-label={t(isCollapsed ? 'nav.expand' : 'nav.collapse', { name: group.name })}
                    onClick={() => toggle(key)}
                  >
                    <span className={isCollapsed ? 'chevron collapsed' : 'chevron'}>
                      <Icon name="chevronDown" size={12} strokeWidth={2.4} />
                    </span>
                    {group.name}
                  </button>
                )}
                <span className="group-actions">
                  <button
                    type="button"
                    className="icon-btn small"
                    aria-label={isCategory ? t('nav.addFolderTo', { name: group.name }) : t('nav.addFolder')}
                    onClick={() => (setEditing({ kind: 'newFolder', categoryId: group.id }), isCollapsed && toggle(key))}
                  >
                    <Icon name="plus" size={14} strokeWidth={2} />
                  </button>
                  {isCategory && (
                    <ItemMenu
                      label={`${t('common.more')}: ${group.name}`}
                      onRename={() => setEditing({ kind: 'renameCategory', id: group.id! })}
                      onDelete={() => setConfirm({ kind: 'category', id: group.id!, name: group.name })}
                    />
                  )}
                </span>
              </div>
              {!isCollapsed && (
                <div className="group-body">
                  {group.folders.map((f) =>
                    editing?.kind === 'renameFolder' && editing.id === f.id ? (
                      <InlineInput
                        key={f.id}
                        label={t('common.rename')}
                        initial={f.name}
                        onCancel={() => setEditing(null)}
                        onSubmit={(name) => (setEditing(null), run.mutate(() => api.renameFolder(f.id, name)))}
                      />
                    ) : (
                      <div
                        key={f.id}
                        className={dropKey === `folder:${f.id}` ? 'folder-row drop' : 'folder-row'}
                        {...dragProps({ type: 'folder', id: f.id })}
                        {...dropProps(`folder:${f.id}`, (d) => d.type === 'folder', () => dropOnFolder(f))}
                      >
                        <Link
                          to={linkFor(String(f.id))}
                          onClick={onNavigate}
                          className={current === String(f.id) ? 'nav-item folder active' : 'nav-item folder'}
                          aria-current={current === String(f.id) ? 'page' : undefined}
                          draggable={false}
                        >
                          <Icon name="folder" size={15} />
                          <span className="ellipsis">{f.name}</span>
                        </Link>
                        <ItemMenu
                          label={`${t('common.more')}: ${f.name}`}
                          onRename={() => setEditing({ kind: 'renameFolder', id: f.id })}
                          onDelete={() => askDeleteFolder(f)}
                        />
                      </div>
                    ),
                  )}
                  {editing?.kind === 'newFolder' && editing.categoryId === group.id && (
                    <InlineInput
                      label={t('nav.newFolder')}
                      onCancel={() => setEditing(null)}
                      onSubmit={(name) => (setEditing(null), run.mutate(() => api.createFolder(name, group.id)))}
                    />
                  )}
                </div>
              )}
            </section>
          )
        })}

        {editing?.kind === 'newCategory' ? (
          <InlineInput
            label={t('nav.newCategory')}
            onCancel={() => setEditing(null)}
            onSubmit={(name) => (setEditing(null), run.mutate(() => api.createCategory(name)))}
          />
        ) : (
          <button type="button" className="nav-item muted add" onClick={() => setEditing({ kind: 'newCategory' })}>
            <Icon name="plus" size={14} /> {t('nav.addCategory')}
          </button>
        )}
        {error && (
          <p className="form-error" role="alert">
            {error}
          </p>
        )}
      </div>
      <div className="sidebar-foot">
        <Link to="/tags" className="nav-item muted" onClick={onNavigate}>
          <Icon name="tag" /> {t('nav.tags')}
        </Link>
        <Link to="/settings" className="nav-item muted" onClick={onNavigate}>
          <Icon name="settings" /> {t('nav.settings')}
        </Link>
      </div>

      {confirm?.kind === 'folder' && (
        <ConfirmDialog
          title={t('nav.deleteFolder.title', { name: confirm.folder.name })}
          confirmLabel={confirm.count > 0 ? t('nav.deleteFolder.confirm', { count: confirm.count }) : t('common.delete')}
          danger
          busy={run.isPending}
          onConfirm={confirmDelete}
          onCancel={() => setConfirm(null)}
        >
          <p>{t('nav.deleteFolder.body', { count: confirm.count })}</p>
          {confirm.count > 0 && (
            <p className="note">
              <Icon name="info" /> {t('nav.deleteFolder.hint')}
            </p>
          )}
        </ConfirmDialog>
      )}
      {confirm?.kind === 'category' && (
        <ConfirmDialog
          title={t('nav.deleteCategory.title', { name: confirm.name })}
          confirmLabel={t('common.delete')}
          danger
          busy={run.isPending}
          onConfirm={confirmDelete}
          onCancel={() => setConfirm(null)}
        >
          <p>{t('nav.deleteCategory.body')}</p>
        </ConfirmDialog>
      )}
    </nav>
  )
}
