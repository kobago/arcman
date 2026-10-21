import { useCallback, useEffect, useMemo, useState, type ReactNode } from 'react'
import { dictionaries, I18nContext, type Lang, type T } from './i18n'

const STORAGE_KEY = 'arcman-lang'

function initialLang(): Lang {
  try {
    const saved = localStorage.getItem(STORAGE_KEY)
    if (saved === 'ja' || saved === 'en') return saved
  } catch {
    // Storage can be blocked; the browser language is a fine fallback.
  }
  return navigator.language.startsWith('ja') ? 'ja' : 'en'
}

export function I18nProvider({ children }: { children: ReactNode }) {
  const [lang, setLangState] = useState<Lang>(initialLang)
  useEffect(() => {
    document.documentElement.lang = lang
  }, [lang])
  const setLang = useCallback((l: Lang) => {
    setLangState(l)
    try {
      localStorage.setItem(STORAGE_KEY, l)
    } catch {
      // Not persisting is acceptable; the choice still applies to this visit.
    }
  }, [])
  const value = useMemo(() => {
    const dict = dictionaries[lang]
    const t: T = (key, vars) =>
      dict[key].replace(/\{(\w+)\}/g, (_, name: string) => String(vars?.[name] ?? `{${name}}`))
    return { lang, setLang, t }
  }, [lang, setLang])
  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>
}
