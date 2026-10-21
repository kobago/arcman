import { useInfiniteQuery, useQuery } from '@tanstack/react-query'
import { useCallback, useEffect, useState } from 'react'
import { Outlet, useSearchParams } from 'react-router'
import { api } from '../api'
import type { GalleryContext } from '../components/Detail'
import { FilterBar } from '../components/FilterBar'
import { Gallery } from '../components/Gallery'
import { Icon } from '../components/Icon'
import { SaveBox } from '../components/SaveBox'
import { Sidebar } from '../components/Sidebar'
import { folderIdOf, parseFilters, toTileQuery, writeFilters, type Filters } from '../filters'
import { folderLabel } from '../folders'
import { useI18n } from '../i18n'

export function GalleryPage() {
  const { t } = useI18n()
  const [sp, setSp] = useSearchParams()
  const filters = parseFilters(sp)
  const [drawerOpen, setDrawerOpen] = useState(false)
  const [saveOpen, setSaveOpen] = useState(false)
  const [searchOpen, setSearchOpen] = useState(!!filters.q)
  const [q, setQ] = useState(filters.q)
  const sidebar = useQuery({ queryKey: ['sidebar'], queryFn: api.sidebar })

  const setFilters = useCallback((f: Filters) => setSp(writeFilters(f)), [setSp])
  const linkFor = (folder: string) => {
    const query = writeFilters({ ...filters, folder }).toString()
    return query ? `/?${query}` : '/'
  }

  useEffect(() => {
    if (q === filters.q) return
    const timer = setTimeout(() => setSp(writeFilters({ ...filters, q }), { replace: true }), 300)
    return () => clearTimeout(timer)
  }, [q, filters, setSp])

  const query = useInfiniteQuery({
    queryKey: ['tiles', toTileQuery(filters, null).toString()],
    queryFn: ({ pageParam }) => api.tiles(toTileQuery(filters, pageParam)),
    initialPageParam: null as string | null,
    getNextPageParam: (page) => page.next_cursor,
    refetchInterval: (q) => (q.state.data?.pages.some((p) => p.tiles.some((x) => x.kind === 'pending')) ? 3000 : false),
  })
  const tiles = query.data?.pages.flatMap((p) => p.tiles) ?? []
  const { hasNextPage, isFetchingNextPage, fetchNextPage } = query
  const loadMore = useCallback(() => {
    if (hasNextPage && !isFetchingNextPage) fetchNextPage()
  }, [hasNextPage, isFetchingNextPage, fetchNextPage])

  const folderId = folderIdOf(filters)
  const title =
    filters.folder === 'unsorted'
      ? t('nav.unsorted')
      : folderId !== null
        ? (sidebar.data && folderLabel(sidebar.data, folderId)?.split(' / ').pop()) ?? ''
        : t('nav.all')
  const destination = folderId !== null ? (folderLabel(sidebar.data, folderId) ?? title) : t('nav.unsorted')
  const filtered = !!(filters.q || filters.tags.length || filters.from || filters.to || filters.media || filters.author)
  const context: GalleryContext = { tiles, hasMore: !!hasNextPage, loadMore }

  return (
    <div className="app">
      <div className={drawerOpen ? 'sidebar-wrap open' : 'sidebar-wrap'}>
        <Sidebar
          current={filters.folder}
          linkFor={linkFor}
          onNavigate={() => setDrawerOpen(false)}
          header={
            <button type="button" className="icon-btn mobile-only" aria-label={t('common.close')} onClick={() => setDrawerOpen(false)}>
              <Icon name="close" size={18} strokeWidth={2} />
            </button>
          }
        />
      </div>
      {drawerOpen && <div className="scrim" onClick={() => setDrawerOpen(false)} aria-hidden="true" />}

      <main className="main">
        <header className="topbar">
          <button type="button" className="icon-btn mobile-only" aria-label={t('nav.open')} onClick={() => setDrawerOpen(true)}>
            <Icon name="menu" size={22} />
          </button>
          <h1 className="mobile-only topbar-title ellipsis">{title}</h1>
          <div className="desktop-only save-slot">
            <SaveBox variant="bar" folderId={folderId} destination={destination} />
          </div>
          <div className={searchOpen ? 'search-field open' : 'search-field'}>
            <Icon name="search" />
            <label htmlFor="search" className="visually-hidden">
              {t('search.label')}
            </label>
            <input id="search" type="search" placeholder={t('search.placeholder')} value={q} onChange={(e) => setQ(e.target.value)} />
          </div>
          <button type="button" className="icon-btn mobile-only" aria-label={t('search.label')} aria-expanded={searchOpen} onClick={() => setSearchOpen((o) => !o)}>
            <Icon name="search" size={20} />
          </button>
          <button type="button" className="icon-btn primary mobile-only" aria-label={t('save.open')} aria-expanded={saveOpen} onClick={() => setSaveOpen((o) => !o)}>
            <Icon name="plus" size={20} strokeWidth={2.2} />
          </button>
        </header>
        {saveOpen && (
          <div className="mobile-only">
            <SaveBox variant="sheet" folderId={folderId} destination={destination} />
          </div>
        )}
        <FilterBar filters={filters} onChange={setFilters} title={<h1 className="desktop-only page-title">{title}</h1>} />

        {query.isError ? (
          <p className="empty">{t('error.generic')}</p>
        ) : query.isSuccess && tiles.length === 0 ? (
          <p className="empty">{filtered ? t('gallery.noMatch') : t('gallery.empty')}</p>
        ) : (
          <Gallery tiles={tiles} search={sp.toString() ? `?${sp}` : ''} hasMore={!!hasNextPage} loadingMore={isFetchingNextPage} onLoadMore={loadMore} />
        )}
      </main>
      <Outlet context={context} />
    </div>
  )
}
