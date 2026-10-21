import { describe, expect, it } from 'vitest'
import { aspectRatio, justify } from './layout'

describe('justify', () => {
  const ratio = (r: number) => r

  it('fills each full row exactly to the container width at equal height', () => {
    const rows = justify([1.5, 1, 1, 2, 0.75, 1], ratio, 1000, 200, 4)
    const full = rows.slice(0, -1)
    expect(full.length).toBeGreaterThan(0)
    for (const row of full) {
      const width = row.tiles.reduce((sum, t) => sum + t.width, 0) + 4 * (row.tiles.length - 1)
      expect(width).toBeCloseTo(1000, 6)
      for (const t of row.tiles) expect(t.width / row.height).toBeCloseTo(t.item, 6)
    }
  })

  it('keeps the last row at the target height instead of stretching it', () => {
    const rows = justify([1, 1, 1, 1, 1, 1], ratio, 1000, 200, 4)
    const last = rows[rows.length - 1]
    expect(last.height).toBe(200)
    expect(last.tiles.map((t) => t.width)).toEqual(last.tiles.map(() => 200))
  })

  it('never shrinks a full row above the target height', () => {
    for (const row of justify([1, 1, 1, 1, 1, 1, 1], ratio, 1000, 200, 4).slice(0, -1)) {
      expect(row.height).toBeLessThanOrEqual(200)
    }
  })

  it('places every item exactly once and in order', () => {
    const items = [0.5, 2.5, 1, 1.3, 0.8, 1.9, 1, 1]
    const placed = justify(items, ratio, 390, 120, 2).flatMap((r) => r.tiles.map((t) => t.item))
    expect(placed).toEqual(items)
  })

  it('returns no rows for no items', () => {
    expect(justify([], ratio, 1000, 200, 4)).toEqual([])
  })
})

describe('aspectRatio', () => {
  it('treats unknown sizes as square', () => {
    expect(aspectRatio(undefined, 300)).toBe(1)
    expect(aspectRatio(400, null)).toBe(1)
  })

  it('clamps panoramas and tall strips so a tile cannot dominate a row', () => {
    expect(aspectRatio(4000, 500)).toBe(2.5)
    expect(aspectRatio(300, 3000)).toBe(0.5)
    expect(aspectRatio(1600, 900)).toBeCloseTo(1.778, 3)
  })
})
