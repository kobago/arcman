import { useQuery } from '@tanstack/react-query'
import type { ReactNode } from 'react'
import { Navigate, Route, Routes, useLocation } from 'react-router'
import { api } from './api'
import { Detail } from './components/Detail'
import { GalleryPage } from './pages/GalleryPage'
import { LoginPage } from './pages/LoginPage'
import { SettingsPage } from './pages/SettingsPage'
import { TagsPage } from './pages/TagsPage'

function RequireSession({ children }: { children: ReactNode }) {
  const location = useLocation()
  const { data: loggedIn } = useQuery({ queryKey: ['session'], queryFn: api.session, staleTime: Infinity })
  if (loggedIn === undefined) return null
  if (!loggedIn) return <Navigate to="/login" replace state={{ from: location.pathname + location.search }} />
  return children
}

export default function App() {
  return (
    <Routes>
      <Route path="/login" element={<LoginPage />} />
      <Route
        path="/"
        element={
          <RequireSession>
            <GalleryPage />
          </RequireSession>
        }
      >
        <Route path="posts/:postId/:index" element={<Detail />} />
      </Route>
      <Route
        path="/tags"
        element={
          <RequireSession>
            <TagsPage />
          </RequireSession>
        }
      />
      <Route
        path="/settings"
        element={
          <RequireSession>
            <SettingsPage />
          </RequireSession>
        }
      />
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  )
}
