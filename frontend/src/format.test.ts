import { describe, expect, it } from 'vitest'
import { formatDateTime, formatDuration, normalizeTagKey, swipeDirection } from './format'

describe('formatDuration', () => {
  it('shows minutes and zero-padded seconds', () => {
    expect(formatDuration(24.666)).toBe('0:25')
    expect(formatDuration(65)).toBe('1:05')
  })

  it('adds hours for long videos', () => {
    expect(formatDuration(3725)).toBe('1:02:05')
  })
})

describe('formatDateTime', () => {
  it('formats in local time as YYYY-MM-DD HH:mm', () => {
    expect(formatDateTime(new Date(2026, 9, 1, 7, 5))).toBe('2026-10-01 07:05')
  })
})

describe('normalizeTagKey', () => {
  it('matches the server rules for case, spaces, hashes and full-width letters', () => {
    for (const name of ['Rust', ' rust ', '#Rust', 'Ｒｕｓｔ', '＃ＲＵＳＴ', '##rust']) {
      expect(normalizeTagKey(name)).toBe('rust')
    }
  })
})

describe('swipeDirection', () => {
  it('ignores short movements', () => {
    expect(swipeDirection(30, 10)).toBeNull()
  })

  it('detects horizontal and vertical swipes', () => {
    expect(swipeDirection(-120, 10)).toBe('left')
    expect(swipeDirection(120, -20)).toBe('right')
    expect(swipeDirection(5, -90)).toBe('up')
    expect(swipeDirection(-10, 90)).toBe('down')
  })

  it('ignores diagonal drags', () => {
    expect(swipeDirection(80, 70)).toBeNull()
  })
})
