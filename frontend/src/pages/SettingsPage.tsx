import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import { Link, useNavigate } from 'react-router'
import { api } from '../api'
import { Icon } from '../components/Icon'
import { errorMessage, useI18n, type Lang } from '../i18n'
import { useTheme, type ThemePref } from '../theme'

function Segmented<T extends string>({ label, value, options, onChange }: { label: string; value: T; options: [T, string][]; onChange: (v: T) => void }) {
  return (
    <div className="setting-row">
      <span>{label}</span>
      <div className="segmented" role="group" aria-label={label}>
        {options.map(([v, text]) => (
          <button key={v} type="button" aria-pressed={value === v} onClick={() => onChange(v)}>
            {text}
          </button>
        ))}
      </div>
    </div>
  )
}

function ApiBaseForm({ initial }: { initial: string }) {
  const { t } = useI18n()
  const qc = useQueryClient()
  const [base, setBase] = useState(initial)
  const save = useMutation({
    mutationFn: () => api.saveSettings(base),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['settings'] }),
  })
  return (
    <form
      className="settings-form"
      onSubmit={(e) => {
        e.preventDefault()
        save.mutate()
      }}
    >
      <label htmlFor="api-base">{t('settings.apiBase')}</label>
      <div className="row">
        <input id="api-base" className="input mono grow" type="url" required value={base} onChange={(e) => (setBase(e.target.value), save.reset())} />
        <button type="submit" className="btn btn-primary" disabled={save.isPending}>
          {t('common.save')}
        </button>
      </div>
      {save.isSuccess && (
        <p role="status" className="small">
          {t('settings.saved')}
        </p>
      )}
      {save.isError && (
        <p className="form-error" role="alert">
          {errorMessage(t, save.error)}
        </p>
      )}
    </form>
  )
}

export function SettingsPage() {
  const { t, lang, setLang } = useI18n()
  const [theme, setTheme] = useTheme()
  const qc = useQueryClient()
  const navigate = useNavigate()
  const settings = useQuery({ queryKey: ['settings'], queryFn: api.settings })
  const logout = useMutation({
    mutationFn: (all: boolean) => (all ? api.logoutAll() : api.logout()),
    onSuccess: () => {
      qc.clear()
      qc.setQueryData(['session'], false)
      navigate('/login', { replace: true })
    },
  })
  const source = settings.data?.fxtwitter_base_url.source

  return (
    <div className="page">
      <div className="page-inner narrow">
        <Link to="/" className="back-link">
          <Icon name="chevronLeft" size={14} strokeWidth={2} /> {t('common.backToGallery')}
        </Link>
        <h1>{t('settings.title')}</h1>

        <section className="settings-card" aria-labelledby="sec-view">
          <h2 id="sec-view">{t('settings.view')}</h2>
          <Segmented<Lang> label={t('settings.language')} value={lang} onChange={setLang} options={[['ja', '日本語'], ['en', 'English']]} />
          <Segmented<ThemePref>
            label={t('settings.theme')}
            value={theme}
            onChange={setTheme}
            options={[
              ['system', t('settings.theme.system')],
              ['light', t('settings.theme.light')],
              ['dark', t('settings.theme.dark')],
            ]}
          />
          <p className="muted small">{t('settings.localOnly')}</p>
        </section>

        <section className="settings-card" aria-labelledby="sec-fetch">
          <h2 id="sec-fetch">{t('settings.fetch')}</h2>
          {settings.data && <ApiBaseForm key={settings.data.fxtwitter_base_url.value} initial={settings.data.fxtwitter_base_url.value} />}
          <p className="muted small source-line">
            {source && <span className="chip-static">{t(`settings.source.${source}`)}</span>}
            {t('settings.apiBaseHint')}
          </p>
        </section>

        <section className="settings-card" aria-labelledby="sec-login">
          <h2 id="sec-login">{t('settings.login')}</h2>
          <div className="row wrap">
            <button type="button" className="btn" onClick={() => logout.mutate(false)}>
              {t('settings.logout')}
            </button>
            <button type="button" className="btn btn-danger" onClick={() => logout.mutate(true)}>
              {t('settings.logoutAll')}
            </button>
          </div>
          <p className="muted small">{t('settings.passwordHint')}</p>
        </section>
      </div>
    </div>
  )
}
