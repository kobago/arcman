import type { Sidebar } from './api'

export function folderLabel(sidebar: Sidebar | undefined, folderId: number | null): string | null {
  if (folderId === null || !sidebar) return null
  for (const c of sidebar.categories) {
    const f = c.folders.find((x) => x.id === folderId)
    if (f) return `${c.name} / ${f.name}`
  }
  return sidebar.uncategorized.find((x) => x.id === folderId)?.name ?? null
}
