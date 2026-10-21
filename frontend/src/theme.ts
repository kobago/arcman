import { useCallback, useState } from 'react'

export type ThemePref = 'system' | 'light' | 'dark'

const STORAGE_KEY = 'arcman-theme'

export function readTheme(): ThemePref {
  try {
    const saved = localStorage.getItem(STORAGE_KEY)
    if (saved === 'light' || saved === 'dark') return saved
  } catch {
    // Blocked storage falls back to following the OS.
  }
  return 'system'
}

export function applyTheme(pref: ThemePref) {
  if (pref === 'system') delete document.documentElement.dataset.theme
  else document.documentElement.dataset.theme = pref
}

export function useTheme(): [ThemePref, (p: ThemePref) => void] {
  const [pref, setPref] = useState(readTheme)
  const set = useCallback((p: ThemePref) => {
    setPref(p)
    applyTheme(p)
    try {
      if (p === 'system') localStorage.removeItem(STORAGE_KEY)
      else localStorage.setItem(STORAGE_KEY, p)
    } catch {
      // The theme still applies for this visit.
    }
  }, [])
  return [pref, set]
}
