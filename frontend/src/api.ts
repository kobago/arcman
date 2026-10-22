export type TileKind = 'photo' | 'video' | 'gif' | 'text' | 'pending' | 'failed'

export type Tile = {
  post_id: number
  kind: TileKind
  index: number
  count: number
  url: string
  saved_at: string
  src?: string
  thumbnail?: string
  width?: number
  height?: number
  duration?: number
  text?: string
  author_name?: string
  author_screen_name?: string
  failure?: string
}

export type TilePage = { tiles: Tile[]; next_cursor: string | null }

export type MediaView = {
  kind: 'photo' | 'video' | 'gif'
  src: string
  thumbnail?: string
  width: number | null
  height: number | null
  duration?: number
}

export type StatusView = {
  url: string
  text: string
  lang: string | null
  posted_at: number | null
  author: { name: string; screen_name: string; avatar: string | null }
  stats: Record<'likes' | 'reposts' | 'quotes' | 'replies' | 'views' | 'bookmarks', number | null>
  media: MediaView[]
  translation?: { text: string; source_lang: string | null }
  quote?: { available: boolean; message?: string; status?: StatusView }
  card?: { url: string; title: string | null; description: string | null; domain: string | null; image: string | null }
  poll?: { choices?: { label: string; count: number; percentage: number }[]; total_votes?: number }
  community_note?: { text: string }
  article?: { content: { title?: string; preview_text?: string }; local_urls: Record<string, string> }
}

export type TagRef = { id: number; name: string; color: string | null }

export type PostDetail = {
  id: number
  status_id: string
  url: string
  state: 'pending' | 'done' | 'failed'
  failure: string | null
  folder_id: number | null
  saved_at: string
  tags: TagRef[]
  status: StatusView | null
}

export type Folder = { id: number; name: string; category_id: number | null }
export type Category = { id: number; name: string; folders: Folder[] }
export type Sidebar = { categories: Category[]; uncategorized: Folder[] }
export type Tag = TagRef & { count: number }
export type Author = { screen_name: string; name: string; count: number }

export type SaveResult = {
  url: string
  outcome: 'created' | 'duplicate' | 'invalid_url' | 'unavailable'
  post_id?: number
  reason?: 'not_found' | 'private'
}

export type PostProgress = { post_id: number; state: 'pending' | 'done' | 'failed'; progress: number | null }

export type Settings = { fxtwitter_base_url: { value: string; source: 'saved' | 'env' | 'default' } }

export class ApiError extends Error {
  readonly status: number
  readonly code: string
  readonly reason?: string

  constructor(status: number, code: string, reason?: string) {
    super(`${status} ${code}`)
    this.status = status
    this.code = code
    this.reason = reason
  }
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(path, {
    method,
    headers: body === undefined ? undefined : { 'Content-Type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  if (!res.ok) {
    const payload = await res.json().catch(() => ({}))
    throw new ApiError(res.status, payload.error ?? 'unknown', payload.reason)
  }
  return (res.status === 204 || res.status === 202 ? undefined : await res.json()) as T
}

export const api = {
  login: (password: string) => request<void>('POST', '/api/login', { password }),
  logout: () => request<void>('POST', '/api/logout'),
  logoutAll: () => request<void>('POST', '/api/logout-all'),
  session: () =>
    request<void>('GET', '/api/session').then(
      () => true,
      (e) => {
        if (e instanceof ApiError && e.status === 401) return false
        throw e
      },
    ),
  tiles: (query: URLSearchParams) => request<TilePage>('GET', `/api/tiles?${query}`),
  savePosts: (urls: string[], folderId: number | null) =>
    request<SaveResult[]>('POST', '/api/posts', { urls, folder_id: folderId }),
  progress: (ids: number[]) => request<PostProgress[]>('GET', `/api/progress?ids=${ids.join(',')}`),
  post: (id: number) => request<PostDetail>('GET', `/api/posts/${id}`),
  movePost: (id: number, folderId: number | null) => request<void>('PATCH', `/api/posts/${id}`, { folder_id: folderId }),
  setPostTags: (id: number, names: string[]) => request<TagRef[]>('PUT', `/api/posts/${id}/tags`, { names }),
  retryPost: (id: number) => request<void>('POST', `/api/posts/${id}/retry`),
  deletePost: (id: number) => request<void>('DELETE', `/api/posts/${id}`),
  authors: () => request<Author[]>('GET', '/api/authors'),
  sidebar: () => request<Sidebar>('GET', '/api/sidebar'),
  createCategory: (name: string) => request<{ id: number }>('POST', '/api/categories', { name }),
  renameCategory: (id: number, name: string) => request<void>('PATCH', `/api/categories/${id}`, { name }),
  deleteCategory: (id: number) => request<void>('DELETE', `/api/categories/${id}`),
  orderCategories: (ids: number[]) => request<void>('PUT', '/api/categories/order', { ids }),
  createFolder: (name: string, categoryId: number | null) =>
    request<{ id: number }>('POST', '/api/folders', { name, category_id: categoryId }),
  folder: (id: number) =>
    request<{ id: number; name: string; category_id: number | null; post_count: number }>('GET', `/api/folders/${id}`),
  renameFolder: (id: number, name: string) => request<void>('PATCH', `/api/folders/${id}`, { name }),
  deleteFolder: (id: number) => request<void>('DELETE', `/api/folders/${id}`),
  orderFolders: (categoryId: number | null, ids: number[]) =>
    request<void>('PUT', '/api/folders/order', { category_id: categoryId, ids }),
  tags: (sort: 'name' | 'count') => request<Tag[]>('GET', `/api/tags?sort=${sort}`),
  updateTag: (id: number, patch: { name?: string; color?: string | null }) =>
    request<void>('PATCH', `/api/tags/${id}`, patch),
  deleteTag: (id: number) => request<void>('DELETE', `/api/tags/${id}`),
  settings: () => request<Settings>('GET', '/api/settings'),
  saveSettings: (fxtwitterBaseUrl: string) =>
    request<void>('PUT', '/api/settings', { fxtwitter_base_url: fxtwitterBaseUrl }),
}
