import { useQuery } from '@tanstack/react-query'
import { lazy, Suspense } from 'react'
import { Navigate, Outlet, Route, Routes, useLocation } from 'react-router'
import { AppShell } from './components/AppShell.tsx'
import { getSession, sessionQueryKey } from './lib/api.ts'

const DashboardPage = lazy(() =>
  import('./pages/DashboardPage.tsx').then((module) => ({ default: module.DashboardPage })),
)
const HostsPage = lazy(() =>
  import('./pages/HostsPage.tsx').then((module) => ({ default: module.HostsPage })),
)
const UpsMonitorPage = lazy(() =>
  import('./pages/UpsMonitorPage.tsx').then((module) => ({ default: module.UpsMonitorPage })),
)
const LoginPage = lazy(() =>
  import('./pages/LoginPage.tsx').then((module) => ({ default: module.LoginPage })),
)

function SessionGate() {
  const location = useLocation()
  const session = useQuery({
    queryKey: sessionQueryKey,
    queryFn: getSession,
    retry: false,
    staleTime: 30_000,
  })

  if (session.isPending) {
    return (
      <main className="grid min-h-screen place-items-center bg-slate-50 text-slate-600">
        <div className="flex items-center gap-3 text-sm">
          <span className="size-2 animate-pulse rounded-full bg-emerald-400" />
          正在连接 NUT Web Manager…
        </div>
      </main>
    )
  }

  if (!session.data?.authenticated) {
    return <Navigate to="/login" replace state={{ from: location.pathname }} />
  }

  return (
    <AppShell>
      <Outlet />
    </AppShell>
  )
}

export default function App() {
  return (
    <Suspense fallback={<PageLoading />}>
      <Routes>
        <Route path="/login" element={<LoginPage />} />
        <Route element={<SessionGate />}>
          <Route index element={<DashboardPage />} />
          <Route path="hosts" element={<HostsPage />} />
          <Route path="ups-monitor" element={<UpsMonitorPage />} />
        </Route>
        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </Suspense>
  )
}

function PageLoading() {
  return (
    <main className="grid min-h-screen place-items-center bg-slate-50 text-sm text-slate-500">
      正在加载页面…
    </main>
  )
}
