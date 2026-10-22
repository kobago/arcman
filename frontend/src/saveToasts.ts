import { createContext, useContext } from 'react'
import type { SaveResult } from './api'

export type Resolved = SaveResult & { folderName?: string | null }

export type SaveToastsApi = {
  start: (urls: string[], destination: string) => number
  finish: (id: number, results: Resolved[]) => void
  fail: (id: number, error: unknown) => void
}

export const SaveToastsContext = createContext<SaveToastsApi | null>(null)

export function useSaveToasts() {
  const api = useContext(SaveToastsContext)
  if (!api) throw new Error('useSaveToasts outside SaveToastProvider')
  return api
}
