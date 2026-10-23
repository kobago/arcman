import { useCallback, useState } from 'react'

// Target row heights per zoom step; index 2 is the original default.
const HEIGHTS = [100, 150, 200, 280, 380]
const NARROW_HEIGHTS = [60, 90, 120, 170, 240]
export const DEFAULT_SIZE = 2
export const MAX_SIZE = HEIGHTS.length - 1

const STORAGE_KEY = 'arcman-gallery-size'

export function rowHeight(size: number, narrow: boolean): number {
  return (narrow ? NARROW_HEIGHTS : HEIGHTS)[size] ?? HEIGHTS[DEFAULT_SIZE]
}

export function readGallerySize(): number {
  try {
    const saved = localStorage.getItem(STORAGE_KEY)
    const size = saved === null ? NaN : Number(saved)
    if (Number.isInteger(size) && size >= 0 && size <= MAX_SIZE) return size
  } catch {
    // Blocked storage falls back to the default size.
  }
  return DEFAULT_SIZE
}

export function useGallerySize(): [number, (s: number) => void] {
  const [size, setSize] = useState(readGallerySize)
  const set = useCallback((s: number) => {
    const next = Math.min(MAX_SIZE, Math.max(0, s))
    setSize(next)
    try {
      localStorage.setItem(STORAGE_KEY, String(next))
    } catch {
      // The size still applies for this visit.
    }
  }, [])
  return [size, set]
}
