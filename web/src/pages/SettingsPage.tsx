import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { LayoutDashboard, MonitorUp, Save, Settings } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'
import {
  getSettings,
  sessionQueryKey,
  settingsQueryKey,
  updateSettings,
} from '../lib/api.ts'
import type { DefaultPage, Session } from '../lib/types.ts'

const pageOptions: Array<{
  value: DefaultPage
  title: string
  description: string
  icon: typeof LayoutDashboard
}> = [
  {
    value: 'overview',
    title: '概览',
    description: '进入系统后展示 UPS 状态与本地保护链路。',
    icon: LayoutDashboard,
  },
  {
    value: 'ups_monitor',
    title: 'UPS 监控',
    description: '进入系统后直接展示 UPS 监控设备、趋势与事件。',
    icon: MonitorUp,
  },
]

export function SettingsPage() {
  const queryClient = useQueryClient()
  const settings = useQuery({ queryKey: settingsQueryKey, queryFn: getSettings })
  const [selectedPage, setSelectedPage] = useState<DefaultPage | null>(null)
  const defaultPage = selectedPage ?? settings.data?.default_page ?? 'overview'

  const save = useMutation({
    mutationFn: () => updateSettings(defaultPage),
    onSuccess: (updated) => {
      queryClient.setQueryData(settingsQueryKey, updated)
      queryClient.setQueryData<Session | null>(sessionQueryKey, (session) =>
        session ? { ...session, default_page: updated.default_page } : session,
      )
      setSelectedPage(null)
      toast.success('默认页面已保存')
    },
    onError: () => toast.error('保存失败，请重试'),
  })

  const changed = settings.data?.default_page !== defaultPage

  return (
    <div>
      <header>
        <p className="mb-2 text-xs font-medium tracking-[0.16em] text-emerald-600/80 uppercase">
          Settings
        </p>
        <h1 className="flex items-center gap-3 text-2xl font-semibold tracking-tight lg:text-3xl">
          <Settings className="text-emerald-600" size={26} /> 系统设置
        </h1>
        <p className="mt-2 text-sm text-slate-500">配置当前 NUT Web Manager 实例的界面偏好。</p>
      </header>

      <section className="mt-8 max-w-3xl rounded-2xl border border-slate-200 bg-white p-5 lg:p-6">
        <div>
          <h2 className="text-sm font-semibold text-slate-800">默认页面</h2>
          <p className="mt-1 text-xs leading-5 text-slate-500">
            登录或访问系统根地址时优先打开的页面。此设置对整个实例生效。
          </p>
        </div>

        {settings.isError && (
          <p className="mt-5 rounded-xl border border-rose-200 bg-rose-50 p-4 text-sm text-rose-700">
            无法读取系统设置，请刷新后重试。
          </p>
        )}

        <div className="mt-5 grid gap-3 sm:grid-cols-2">
          {pageOptions.map(({ value, title, description, icon: Icon }) => {
            const selected = defaultPage === value
            return (
              <label
                key={value}
                className={`cursor-pointer rounded-xl border p-4 transition ${
                  selected
                    ? 'border-emerald-300 bg-emerald-50/70'
                    : 'border-slate-200 hover:border-slate-300'
                } ${settings.isPending ? 'pointer-events-none opacity-60' : ''}`}
              >
                <span className="flex items-start gap-3">
                  <input
                    type="radio"
                    name="default-page"
                    value={value}
                    checked={selected}
                    onChange={() => setSelectedPage(value)}
                    className="mt-1 accent-emerald-600"
                  />
                  <span>
                    <span className="flex items-center gap-2 text-sm font-medium text-slate-800">
                      <Icon size={17} className="text-emerald-600" /> {title}
                    </span>
                    <span className="mt-2 block text-xs leading-5 text-slate-500">{description}</span>
                  </span>
                </span>
              </label>
            )
          })}
        </div>

        <div className="mt-6 flex justify-end border-t border-slate-200 pt-5">
          <button
            type="button"
            disabled={!changed || save.isPending || settings.isPending || settings.isError}
            onClick={() => save.mutate()}
            className="flex items-center gap-2 rounded-xl bg-emerald-300 px-4 py-2.5 text-sm font-semibold text-emerald-950 transition hover:bg-emerald-200 disabled:cursor-not-allowed disabled:opacity-50"
          >
            <Save size={16} /> {save.isPending ? '保存中…' : '保存设置'}
          </button>
        </div>
      </section>
    </div>
  )
}
