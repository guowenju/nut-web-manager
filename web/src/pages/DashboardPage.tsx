import { useQuery } from '@tanstack/react-query'
import {
  Activity,
  AlertTriangle,
  BatteryCharging,
  Clock3,
  Gauge,
  Network,
  PlugZap,
  Server,
} from 'lucide-react'
import { dashboardQueryKey, getDashboard, hostsQueryKey, listHosts } from '../lib/api.ts'
import type { ProtectionHealth } from '../lib/types.ts'

export function DashboardPage() {
  const hosts = useQuery({ queryKey: hostsQueryKey, queryFn: listHosts })
  const dashboard = useQuery({
    queryKey: dashboardQueryKey,
    queryFn: getDashboard,
    refetchInterval: 5_000,
    retry: 1,
  })
  const serverCount = hosts.data?.filter((host) => host.role === 'server').length ?? 0
  const clientCount = hosts.data?.filter((host) => host.role === 'client').length ?? 0
  const snapshot = dashboard.data
  const ups = snapshot?.ups
  const onBattery = ups?.status_flags.includes('OB') ?? false
  const lowBattery = ups?.status_flags.includes('LB') ?? false

  return (
    <div>
      <header>
        <div>
          <p className="mb-2 text-xs font-medium tracking-[0.16em] text-emerald-600/80 uppercase">Overview</p>
          <h1 className="text-2xl font-semibold tracking-tight text-slate-900 lg:text-3xl">系统概览</h1>
          <p className="mt-2 text-sm text-slate-500">实时查看 UPS 数据与本地保护链路。</p>
        </div>
      </header>

      {(onBattery || lowBattery) && ups && (
        <PowerFailureAlert
          lowBattery={lowBattery}
          chargePercent={ups.charge_percent}
          runtimeSeconds={ups.runtime_seconds}
          policy={shutdownTriggerPolicy(snapshot?.server)}
        />
      )}

      <section className="mt-8 grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
        <MetricCard label="管理主机" value={hosts.data?.length ?? 0} icon={Server} tone="emerald" />
        <MetricCard label="NUT Server" value={serverCount} icon={BatteryCharging} tone="cyan" />
        <MetricCard label="NUT Client" value={clientCount} icon={Network} tone="violet" />
        <MetricCard label="保护状态" value={healthLabel(snapshot?.protection)} icon={PlugZap} tone={healthTone(snapshot?.protection)} small />
      </section>

      {dashboard.isError && (
        <div className="mt-6 flex items-center gap-3 rounded-xl border border-rose-400/15 bg-rose-400/[0.05] p-4 text-sm text-rose-700">
          <AlertTriangle size={18} /> 无法读取概览状态，将自动重试。
        </div>
      )}

      <section className="mt-6 grid gap-6 xl:grid-cols-[1.5fr_1fr]">
        <div className="rounded-2xl border border-slate-200 bg-white p-5 lg:p-6">
          <div className="flex items-center justify-between gap-4">
            <div>
              <h2 className="text-sm font-semibold text-slate-800">UPS 实时状态</h2>
              <p className="mt-1 text-xs text-slate-500">每 5 秒从 NUT TCP 3493 刷新</p>
            </div>
            {ups && <StatusBadge health={snapshot?.protection} text={ups.status_flags.join(' ') || 'Unknown'} />}
          </div>

          {ups ? (
            <div className="mt-6">
              <div className="flex flex-col gap-5 rounded-xl border border-emerald-300/10 bg-emerald-300/[0.025] p-5 sm:flex-row sm:items-center">
                <div className="grid size-14 shrink-0 place-items-center rounded-2xl bg-emerald-300/10 text-emerald-600">
                  <BatteryCharging size={27} strokeWidth={1.6} />
                </div>
                <div className="min-w-0 flex-1">
                  <p className="truncate text-base font-semibold text-slate-900">{[ups.manufacturer, ups.model].filter(Boolean).join(' ') || 'USB UPS'}</p>
                  <p className="mt-1 text-xs text-slate-500">{powerLabel(ups.power_source)} · {snapshot?.server?.ups_name}@{snapshot?.server?.listen_address}:{snapshot?.server?.listen_port}</p>
                </div>
                <div className="text-left sm:text-right">
                  <p className="text-3xl font-semibold tracking-tight text-emerald-600">{value(ups.charge_percent, '%')}</p>
                  <p className="mt-1 text-xs text-slate-500">电池电量</p>
                </div>
              </div>
              <div className="mt-4 grid gap-3 sm:grid-cols-3">
                <DataCard icon={Gauge} label="当前负载" value={value(ups.load_percent, '%')} />
                <DataCard icon={Clock3} label="预计续航" value={runtime(ups.runtime_seconds)} />
                <DataCard icon={PlugZap} label="UPS 估算输出功率" value={watts(ups.raw['ups.realpower'])} />
              </div>
              <div className={`mt-4 rounded-xl border p-4 text-xs leading-5 ${snapshot?.server && snapshot.server.apply_state !== 'applied' ? 'border-amber-300 bg-amber-50 font-semibold text-amber-800' : 'border-slate-200 bg-slate-50 text-slate-600'}`}>
                <p>当前关机策略：{shutdownPolicy(snapshot?.server)}</p>
              </div>
            </div>
          ) : (
            <EmptyState configured={snapshot?.protection !== 'unconfigured'} loading={dashboard.isPending} />
          )}
        </div>

        <div className="rounded-2xl border border-slate-200 bg-white p-5 lg:p-6">
          <div className="flex items-center gap-2">
            <Activity size={17} className="text-emerald-600" />
            <h2 className="text-sm font-semibold text-slate-800">保护链路</h2>
          </div>
          <div className="mt-6 space-y-3">
            <StatusRow label="管理连接（SSH）" status={managementLabel(snapshot?.management)} active={snapshot?.management === 'connected'} />
            <StatusRow label="UPS 驱动" status={serviceLabel(snapshot?.services?.driver_active)} active={snapshot?.services?.driver_active} />
            <StatusRow label="NUT Server" status={serviceLabel(snapshot?.services?.server_active)} active={snapshot?.services?.server_active} />
            <StatusRow label="upsmon primary" status={serviceLabel(snapshot?.services?.monitor_active)} active={snapshot?.services?.monitor_active} />
            <StatusRow label="最近完整验证" status={snapshot?.last_verified_at ? dateTime(snapshot.last_verified_at) : '从未'} active={Boolean(snapshot?.last_verified_at)} />
          </div>
        </div>
      </section>
    </div>
  )
}

const tones = {
  emerald: 'bg-emerald-300/10 text-emerald-600',
  cyan: 'bg-cyan-300/10 text-cyan-700',
  violet: 'bg-violet-300/10 text-violet-600',
  slate: 'bg-slate-300/10 text-slate-700',
  amber: 'bg-amber-300/10 text-amber-600',
}

function MetricCard({ label, value: metric, icon: Icon, tone, small = false }: { label: string; value: string | number; icon: typeof Server; tone: keyof typeof tones; small?: boolean }) {
  return <article className="rounded-2xl border border-slate-200 bg-white p-5"><div className="flex items-center justify-between"><p className="text-xs font-medium text-slate-500">{label}</p><div className={`grid size-8 place-items-center rounded-lg ${tones[tone]}`}><Icon size={16} strokeWidth={1.8} /></div></div><p className={`mt-5 font-semibold tracking-tight text-slate-900 ${small ? 'text-xl' : 'text-3xl'}`}>{metric}</p></article>
}

function DataCard({ icon: Icon, label, value: cardValue }: { icon: typeof Gauge; label: string; value: string }) {
  return <div className="rounded-xl border border-slate-200 bg-slate-50 p-4"><Icon size={16} className="text-cyan-700" /><p className="mt-4 text-lg font-semibold text-slate-800">{cardValue}</p><p className="mt-1 text-xs text-slate-500">{label}</p></div>
}

function StatusRow({ label, status, active = false }: { label: string; status: string; active?: boolean }) {
  return <div className="flex items-center justify-between gap-4 border-b border-slate-200 py-3 text-xs last:border-0"><span className="text-slate-500">{label}</span><span className={`flex items-center gap-2 text-right ${active ? 'text-emerald-600' : 'text-slate-600'}`}><span className={`size-1.5 shrink-0 rounded-full ${active ? 'bg-emerald-400' : 'bg-slate-600'}`} />{status}</span></div>
}

function StatusBadge({ health, text }: { health?: ProtectionHealth; text: string }) {
  const active = health === 'active'
  return <span className={`rounded-full border px-2.5 py-1 text-[10px] font-medium tracking-wider uppercase ${active ? 'border-emerald-300/15 bg-emerald-300/[0.05] text-emerald-600' : 'border-amber-300/15 bg-amber-300/[0.05] text-amber-600'}`}>{text}</span>
}

function EmptyState({ configured, loading }: { configured: boolean; loading: boolean }) {
  return <div className="mt-6 grid min-h-64 place-items-center rounded-xl border border-dashed border-slate-200 bg-slate-50 px-6 text-center"><div className="max-w-sm"><Network size={28} className="mx-auto text-slate-600" /><h3 className="mt-4 text-sm font-medium text-slate-700">{loading ? '正在读取 UPS 状态…' : configured ? '暂时无法读取 UPS 数据' : '尚未应用 NUT Server'}</h3><p className="mt-2 text-xs leading-5 text-slate-500">{configured ? '请检查 TCP 3493 可达性和 NUT Server 服务状态。' : '完成 USB 扫描并应用 Server 配置后，这里会显示实时数据。'}</p></div></div>
}

function PowerFailureAlert({ lowBattery, chargePercent, runtimeSeconds, policy }: { lowBattery: boolean; chargePercent: number | null; runtimeSeconds: number | null; policy: string }) {
  const tone = lowBattery
    ? 'border-rose-300 bg-rose-50 text-rose-900 shadow-[0_10px_30px_rgba(244,63,94,0.10)]'
    : 'border-amber-300 bg-amber-50 text-amber-950 shadow-[0_10px_30px_rgba(245,158,11,0.10)]'
  return (
    <section role="alert" aria-live="assertive" className={`mt-6 rounded-2xl border p-5 ${tone}`}>
      <div className="flex items-start gap-4">
        <div className={`grid size-11 shrink-0 place-items-center rounded-xl ${lowBattery ? 'bg-rose-100 text-rose-700' : 'bg-amber-100 text-amber-700'}`}>
          <AlertTriangle size={23} />
        </div>
        <div>
          <h2 className="text-base font-semibold">{lowBattery ? '电池电量低：关机保护即将触发' : '市电已断开：UPS 正在使用电池供电'}</h2>
          <p className="mt-1.5 text-sm opacity-80">当前电量 {value(chargePercent, '%')}，预计续航 {runtime(runtimeSeconds)}。关机策略：{policy}。</p>
          <p className="mt-2 text-xs opacity-65">状态每 5 秒刷新；恢复市电后，本地 NUT 会自动取消尚未到期的断电计时。</p>
        </div>
      </div>
    </section>
  )
}

function healthLabel(health?: ProtectionHealth) { return ({ active: 'Active', degraded: 'Degraded', unknown: 'Unknown', unconfigured: '未配置' } as const)[health ?? 'unknown'] }
function healthTone(health?: ProtectionHealth): keyof typeof tones { return health === 'active' ? 'emerald' : health === 'degraded' ? 'amber' : 'slate' }
function managementLabel(value?: string) { return ({ connected: '已连接', disconnected: '已断开', host_key_mismatch: 'Host Key 异常', authentication_failed: '认证失败', unknown: 'Unknown' } as Record<string, string>)[value ?? 'unknown'] }
function serviceLabel(active?: boolean) { return active === undefined ? 'Unknown' : active ? '运行中' : '未运行' }
function powerLabel(source: string) { return ({ mains: '市电供电', battery: '电池供电', bypass: '旁路供电', off: '输出关闭', other: '其它状态', unknown: '供电状态未知' } as Record<string, string>)[source] }
function value(input: number | null, suffix: string) { return input === null ? '—' : `${Math.round(input)}${suffix}` }
function runtime(seconds: number | null) { if (seconds === null) return '—'; const minutes = Math.round(seconds / 60); return minutes >= 60 ? `${Math.floor(minutes / 60)} 小时 ${minutes % 60} 分` : `${minutes} 分钟` }
function watts(raw?: string) { const parsed = raw === undefined ? Number.NaN : Number(raw); return Number.isFinite(parsed) ? `${Math.round(parsed)} W` : '—' }
function dateTime(value: string) { return new Intl.DateTimeFormat('zh-CN', { month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit', second: '2-digit' }).format(new Date(value)) }
function shutdownPolicy(server: import('../lib/types.ts').NutServerRecord | null | undefined) {
  if (!server) return '尚未配置'
  const policy = shutdownTriggerPolicy(server)
  const output = server.shutdown.powerdown_enabled ? '；关机后切断 UPS 全部输出' : '；UPS 输出保持开启'
  return server.apply_state === 'applied' ? `${policy}${output}` : `待重新应用：${policy}${output}`
}
function shutdownTriggerPolicy(server: import('../lib/types.ts').NutServerRecord | null | undefined) {
  if (!server) return '尚未配置'
  return server.shutdown.trigger_mode === 'battery_level'
    ? `电池电量低于 ${server.shutdown.battery_level_percent}%`
    : `市电断电持续 ${server.shutdown.on_battery_seconds} 秒，或电量低于 ${server.shutdown.battery_level_percent}%`
}
