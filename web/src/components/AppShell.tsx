import { useMutation, useQueryClient } from '@tanstack/react-query'
import {
  Boxes,
  LayoutDashboard,
  LogOut,
  Network,
  Server,
  MonitorUp,
} from 'lucide-react'
import type { ReactNode } from 'react'
import { NavLink, useNavigate } from 'react-router'
import { toast } from 'sonner'
import { logout, sessionQueryKey } from '../lib/api.ts'
interface AppShellProps {
  children: ReactNode
}

const navItems = [
  { to: '/', label: '概览', icon: LayoutDashboard, end: true },
  { to: '/hosts', label: '主机', icon: Server, end: false },
  { to: '/ups-monitor', label: 'UPS 监控', icon: MonitorUp, end: false },
]

export function AppShell({ children }: AppShellProps) {
  const queryClient = useQueryClient()
  const navigate = useNavigate()
  const logoutMutation = useMutation({
    mutationFn: logout,
    onSuccess: () => {
      queryClient.setQueryData(sessionQueryKey, null)
      queryClient.clear()
      navigate('/login', { replace: true })
    },
    onError: () => toast.error('退出失败，请重试'),
  })

  return (
    <div className="min-h-screen bg-slate-50 text-slate-900">
      <aside className="fixed inset-y-0 left-0 z-20 hidden w-64 flex-col border-r border-slate-200 bg-white px-4 py-5 md:flex">
        <Brand />
        <nav className="mt-9 space-y-1.5" aria-label="主导航">
          {navItems.map(({ to, label, icon: Icon, end }) => (
            <NavLink
              key={to}
              to={to}
              end={end}
              className={({ isActive }) =>
                `flex items-center gap-3 rounded-xl px-3 py-2.5 text-sm font-medium transition ${
                  isActive
                    ? 'bg-emerald-50 text-emerald-700'
                    : 'text-slate-500 hover:bg-slate-100 hover:text-slate-900'
                }`
              }
            >
              <Icon size={18} strokeWidth={1.8} />
              {label}
            </NavLink>
          ))}
        </nav>

        <div className="mt-auto border-t border-slate-200 pt-3">
          <button
            type="button"
            disabled={logoutMutation.isPending}
            onClick={() => logoutMutation.mutate()}
            className="flex w-full items-center gap-2 rounded-lg px-3 py-2.5 text-sm text-slate-500 transition hover:bg-slate-100 hover:text-slate-800 disabled:opacity-50"
          >
            <LogOut size={15} />
            退出
          </button>
        </div>
      </aside>

      <header className="sticky top-0 z-20 border-b border-slate-200 bg-white/95 px-4 py-3 backdrop-blur md:hidden">
        <div className="flex items-center justify-between">
          <Brand compact />
          <nav className="flex gap-1" aria-label="移动端导航">
            {navItems.map(({ to, label, icon: Icon, end }) => (
              <NavLink
                key={to}
                to={to}
                end={end}
                aria-label={label}
                className={({ isActive }) =>
                  `rounded-lg p-2 ${isActive ? 'bg-emerald-50 text-emerald-700' : 'text-slate-500'}`
                }
              >
                <Icon size={19} />
              </NavLink>
            ))}
          </nav>
        </div>
      </header>

      <main className="md:pl-64">
        <div className="mx-auto max-w-7xl px-5 py-7 lg:px-9 lg:py-9">{children}</div>
      </main>
    </div>
  )
}

function Brand({ compact = false }: { compact?: boolean }) {
  return (
    <div className="flex items-center gap-3 px-1">
      <div className="relative grid size-9 place-items-center overflow-hidden rounded-xl bg-emerald-50 text-emerald-600">
        <Network size={20} strokeWidth={1.7} />
        <span className="absolute -right-1 -bottom-1 size-2.5 rounded-full bg-emerald-500 ring-2 ring-white" />
      </div>
      {!compact && (
        <div>
          <p className="text-sm font-semibold tracking-wide text-slate-900">NUT Web Manager</p>
          <p className="mt-0.5 flex items-center gap-1 text-[10px] uppercase tracking-[0.18em] text-slate-500">
            <Boxes size={10} /> LAN Control Plane
          </p>
        </div>
      )}
    </div>
  )
}
