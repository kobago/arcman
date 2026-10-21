import { describe, expect, it } from 'vitest'
import { folderIdOf, parseFilters, toTileQuery, writeFilters, type Filters } from './filters'

const defaults: Filters = { folder: 'all', q: '', tags: [], from: '', to: '', media: '', author: '', order: 'desc' }

describe('parseFilters / writeFilters', () => {
  it('reads an empty URL as the default view', () => {
    expect(parseFilters(new URLSearchParams())).toEqual(defaults)
  })

  it('round-trips every filter through the URL', () => {
    const f: Filters = { folder: '3', q: '猫', tags: [1, 2], from: '2026-09-01', to: '2026-09-30', media: 'photo', author: 'alice', order: 'asc' }
    expect(parseFilters(writeFilters(f))).toEqual(f)
  })

  it('leaves defaults out of the URL', () => {
    expect(writeFilters(defaults).toString()).toBe('')
  })

  it('ignores malformed values from a hand-edited URL', () => {
    const f = parseFilters(new URLSearchParams('tags=1,x,-2,3&media=audio&from=yesterday&order=sideways'))
    expect(f.tags).toEqual([1, 3])
    expect(f.media).toBe('')
    expect(f.from).toBe('')
    expect(f.order).toBe('desc')
  })
})

describe('toTileQuery', () => {
  it('turns the inclusive date range into local-midnight bounds with an exclusive end', () => {
    const sp = toTileQuery({ ...defaults, from: '2026-09-01', to: '2026-09-30' }, null)
    expect(sp.get('from')).toBe(new Date(2026, 8, 1).toISOString())
    expect(sp.get('to')).toBe(new Date(2026, 9, 1).toISOString())
  })

  it('passes the cursor and trims the keyword', () => {
    const sp = toTileQuery({ ...defaults, q: '  cat ' }, '2026-10-01T00:00:00.000Z|5', 10)
    expect(sp.get('q')).toBe('cat')
    expect(sp.get('cursor')).toBe('2026-10-01T00:00:00.000Z|5')
    expect(sp.get('limit')).toBe('10')
  })
})

describe('folderIdOf', () => {
  it('returns the folder id only for a real folder', () => {
    expect(folderIdOf({ ...defaults, folder: '7' })).toBe(7)
    expect(folderIdOf({ ...defaults, folder: 'unsorted' })).toBeNull()
    expect(folderIdOf(defaults)).toBeNull()
  })
})
