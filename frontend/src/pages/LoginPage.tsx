import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useState, type FormEvent } from 'react'
import { useLocation, useNavigate } from 'react-router'
import { api, ApiError } from '../api'
import { Monogram, Wordmark } from '../components/Logo'
import { errorMessage, useI18n } from '../i18n'

export function LoginPage() {
  const { t } = useI18n()
  const qc = useQueryClient()
  const navigate = useNavigate()
  const location = useLocation()
  const [password, setPassword] = useState('')
  const login = useMutation({
    mutationFn: () => api.login(password),
    onSuccess: () => {
      qc.setQueryData(['session'], true)
      navigate((location.state as { from?: string } | null)?.from ?? '/', { replace: true })
    },
  })
  const error = login.error
  const message =
    error instanceof ApiError && error.status === 401
      ? t('login.wrong')
      : error instanceof ApiError && error.status === 429
        ? t('login.locked')
        : error
          ? errorMessage(t, error)
          : null

  const submit = (e: FormEvent) => {
    e.preventDefault()
    if (password) login.mutate()
  }

  return (
    <div className="login">
      <h1 className="login-logo">
        <span role="img" aria-label="ARCMAN">
          <Monogram size={88} ground="var(--bg)" metal />
          <Wordmark height={30} metal />
        </span>
      </h1>
      <form onSubmit={submit} className="login-form">
        <label htmlFor="password" className="field-label">
          {t('login.password')}
        </label>
        <input
          id="password"
          className={message ? 'input large invalid' : 'input large'}
          type="password"
          autoComplete="current-password"
          autoFocus
          aria-describedby={message ? 'login-error' : undefined}
          value={password}
          onChange={(e) => setPassword(e.target.value)}
        />
        {message && (
          <p id="login-error" className="form-error" role="alert">
            {message}
          </p>
        )}
        <button type="submit" className="btn btn-primary large" disabled={login.isPending}>
          {t('login.submit')}
        </button>
      </form>
    </div>
  )
}
