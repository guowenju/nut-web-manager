import { useQuery } from '@tanstack/react-query'
import {
  Activity, AlertTriangle, BatteryCharging, Clock3, Gauge, ListFilter,
  PlugZap, Server, Thermometer, TrendingUp, Unplug, Zap,
} from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { UpsMonitorSourcesDialog } from '../components/UpsMonitorSourcesDialog.tsx'
import { TimeSeriesChart } from '../components/TimeSeriesChart.tsx'
import {
  getUpsMonitorEvents, getUpsMonitorHistory, getUpsMonitorOverview, getUpsMonitorSnapshot,
  upsMonitorQueryKey,
} from '../lib/api.ts'
import type { UpsMonitorDevice, UpsMonitorSample } from '../lib/types.ts'

const selectionKey = 'nwm-ups-monitor-device'
const ranges = ['24h', '7d', '30d', '90d'] as const
const emptyDevices: UpsMonitorDevice[] = []

export function UpsMonitorPage() {
  const overview = useQuery({ queryKey: [...upsMonitorQueryKey, 'overview'], queryFn: getUpsMonitorOverview, refetchInterval: 5_000 })
  const devices = overview.data?.devices ?? emptyDevices
  const [requestedId, setRequestedId] = useState(() => localStorage.getItem(selectionKey) ?? '')
  const selectedId = devices.some((device) => device.id === requestedId) ? requestedId : devices[0]?.id ?? ''
  const [range, setRange] = useState<(typeof ranges)[number]>('24h')
  const [tab, setTab] = useState<'details' | 'events' | 'raw'>('details')
  const [search, setSearch] = useState('')

  useEffect(() => { if (selectedId) localStorage.setItem(selectionKey, selectedId) }, [selectedId])

  const snapshot = useQuery({
    queryKey: [...upsMonitorQueryKey, 'snapshot', selectedId], queryFn: () => getUpsMonitorSnapshot(selectedId),
    enabled: Boolean(selectedId), refetchInterval: 5_000,
  })
  const history = useQuery({
    queryKey: [...upsMonitorQueryKey, 'history', selectedId, range], queryFn: () => getUpsMonitorHistory(selectedId, range),
    enabled: Boolean(selectedId), staleTime: 30_000, refetchInterval: 60_000,
  })
  const events = useQuery({
    queryKey: [...upsMonitorQueryKey, 'events', selectedId], queryFn: () => getUpsMonitorEvents(selectedId),
    enabled: Boolean(selectedId), refetchInterval: 10_000,
  })

  const online = devices.filter((device) => device.online).length
  const onBattery = devices.filter((device) => device.status_flags.includes('OB')).length
  const abnormal = devices.filter(isAbnormal).length
  const current = snapshot.data

  return (
    <div>
      <header className="flex flex-col justify-between gap-5 sm:flex-row sm:items-end">
        <div><p className="mb-2 text-xs font-medium tracking-[0.16em] text-emerald-600/80 uppercase">Read-only NUT telemetry</p><h1 className="text-2xl font-semibold tracking-tight lg:text-3xl">UPS 监控</h1><p className="mt-2 text-sm text-slate-500">集中展示标准 NUT Server 的实时状态与历史趋势。</p></div>
        <UpsMonitorSourcesDialog sources={overview.data?.sources ?? []} />
      </header>

      <section className="mt-8 grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
        <SummaryCard label="已发现 UPS" value={devices.length} icon={Server} tone="slate" />
        <SummaryCard label="在线设备" value={online} icon={Activity} tone="emerald" />
        <SummaryCard label="电池供电" value={onBattery} icon={BatteryCharging} tone={onBattery ? 'amber' : 'cyan'} />
        <SummaryCard label="异常状态" value={abnormal} icon={AlertTriangle} tone={abnormal ? 'rose' : 'emerald'} />
      </section>

      {overview.isError && <Notice text="无法读取 UPS 监控数据，页面将自动重试。" />}
      {!overview.isPending && devices.length === 0 ? (
        <section className="mt-6 grid min-h-80 place-items-center rounded-2xl border border-dashed border-slate-300 bg-white p-8 text-center"><div className="max-w-md"><Unplug size={36} className="mx-auto text-slate-400" /><h2 className="mt-4 font-semibold text-slate-800">还没有监控设备</h2><p className="mt-2 text-sm leading-6 text-slate-500">打开“数据源”，添加 NAS 的地址和 NUT 端口。后台将自动发现并采集 UPS。</p></div></section>
      ) : (
        <>
          <section className="mt-6">
            <div className="mb-3 flex items-center justify-between"><h2 className="text-sm font-semibold text-slate-800">全部设备</h2><span className="text-xs text-slate-500">每 5 秒更新</span></div>
            <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
              {devices.map((device) => <DeviceCard key={device.id} device={device} selected={device.id === selectedId} onClick={() => setRequestedId(device.id)} />)}
            </div>
          </section>

          {selectedId && <section className="mt-6 overflow-hidden rounded-2xl border border-slate-200 bg-white">
            <div className="border-b border-slate-200 p-5 lg:p-6">
              <div className="flex flex-col justify-between gap-4 sm:flex-row sm:items-start">
                <div><p className="text-xs text-slate-500">{current?.device.source_name}</p><h2 className="mt-1 text-xl font-semibold text-slate-900">{deviceTitle(current?.device)}</h2><p className="mt-1 text-xs text-slate-500">最近采集：{dateTime(current?.device.observed_at)}</p></div>
                <StatusBadge device={current?.device} />
              </div>
              <div className="mt-6 grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
                <Metric icon={BatteryCharging} label="电池电量" value={percent(current?.device.charge_percent)} />
                <Metric icon={Clock3} label="预计续航" value={runtime(current?.device.runtime_seconds, current?.device.runtime_capped)} />
                <Metric icon={Gauge} label="当前负载" value={percent(current?.device.load_percent)} />
                <Metric icon={Thermometer} label="电池温度" value={unit(current?.battery_temperature, '°C')} />
              </div>
              <div className="mt-3 grid gap-3 sm:grid-cols-2">
                <Metric icon={Zap} label="输入电压" value={unit(current?.input_voltage, ' V')} compact />
                <Metric icon={PlugZap} label="输出电压" value={unit(current?.output_voltage, ' V')} compact />
              </div>
            </div>

            <div className="border-b border-slate-200 px-5">
              <nav className="flex gap-5 overflow-x-auto">{([['details', '趋势与详情'], ['events', '事件记录'], ['raw', '原始变量']] as const).map(([value, label]) => <button key={value} type="button" onClick={() => setTab(value)} className={`border-b-2 py-4 text-sm font-medium ${tab === value ? 'border-emerald-500 text-emerald-700' : 'border-transparent text-slate-500'}`}>{label}</button>)}</nav>
            </div>

            <div className="p-5 lg:p-6">
              {tab === 'details' && <DetailsTab samples={history.data ?? []} range={range} setRange={setRange} raw={current?.raw ?? {}} />}
              {tab === 'events' && <EventsTab events={events.data ?? []} />}
              {tab === 'raw' && <RawTab raw={current?.raw ?? {}} search={search} setSearch={setSearch} />}
            </div>
          </section>}
        </>
      )}
    </div>
  )
}

function DetailsTab({ samples, range, setRange, raw }: { samples: UpsMonitorSample[]; range: string; setRange: (range: typeof ranges[number]) => void; raw: Record<string, string> }) {
  return <div>
    <div className="flex flex-col justify-between gap-3 sm:flex-row sm:items-center"><div><h3 className="flex items-center gap-2 text-sm font-semibold"><TrendingUp size={17} className="text-emerald-600" />历史趋势</h3><p className="mt-1 text-xs text-slate-500">后台每分钟保存一个采样点</p></div><div className="flex rounded-lg bg-slate-100 p-1">{ranges.map((value) => <button key={value} type="button" onClick={() => setRange(value)} className={`rounded-md px-2.5 py-1.5 text-xs ${range === value ? 'bg-white font-medium text-slate-800 shadow-sm' : 'text-slate-500'}`}>{value}</button>)}</div></div>
    <div className="mt-5 grid gap-4 xl:grid-cols-2">
      <TimeSeriesChart title="电池与负载" series={[chartSeries(samples, 'charge_percent', '电量', '#059669'), chartSeries(samples, 'load_percent', '负载', '#0891b2')]} unit="%" />
      <TimeSeriesChart title="输入与输出电压" series={[chartSeries(samples, 'input_voltage', '输入', '#7c3aed'), chartSeries(samples, 'output_voltage', '输出', '#ea580c')]} unit=" V" />
      <TimeSeriesChart title="电池温度" series={[chartSeries(samples, 'battery_temperature', '温度', '#dc2626')]} unit="°C" />
      <TimeSeriesChart title="预计续航" series={[chartSeries(samples, 'runtime_seconds', '续航', '#2563eb', (value) => value == null ? null : value / 60)]} unit=" 分钟" valueFormatter={(value) => runtime(Math.round(value * 60))} emptyLabel="设备未提供可靠的预计续航" />
    </div>
    <h3 className="mt-8 text-sm font-semibold text-slate-800">分类详情</h3>
    <div className="mt-4 grid gap-4 lg:grid-cols-2">
      <VariableGroup title="电池" entries={group(raw, ['battery.'])} />
      <VariableGroup title="输入与输出" entries={group(raw, ['input.', 'output.'])} />
      <VariableGroup title="设备与 UPS" entries={group(raw, ['device.', 'ups.'])} />
      <VariableGroup title="插座" entries={group(raw, ['outlet.'])} />
      <VariableGroup title="驱动" entries={group(raw, ['driver.'])} />
    </div>
  </div>
}

function chartSeries(samples: UpsMonitorSample[], key: keyof UpsMonitorSample, name: string, color: string, transform: (value: number | null) => number | null = (value) => value) {
  return { name, color, points: samples.map((sample) => [sample.observed_at, transform(typeof sample[key] === 'number' ? sample[key] : null)] as [string, number | null]) }
}

function EventsTab({ events }: { events: Array<{ id: number; occurred_at: string; severity: string; message: string; status_flags: string[] }> }) {
  if (!events.length) return <p className="rounded-xl border border-dashed border-slate-200 p-10 text-center text-sm text-slate-500">暂无状态变化事件</p>
  return <div className="space-y-2">{events.map((event) => <div key={event.id} className="flex items-start gap-3 rounded-xl border border-slate-200 p-4"><span className={`mt-1 size-2 shrink-0 rounded-full ${event.severity === 'critical' ? 'bg-rose-500' : event.severity === 'warning' ? 'bg-amber-500' : 'bg-emerald-500'}`} /><div className="min-w-0 flex-1"><p className="text-sm text-slate-800">{event.message}</p><p className="mt-1 text-xs text-slate-500">{dateTime(event.occurred_at)} · {event.status_flags.join(' ') || '—'}</p></div></div>)}</div>
}

function RawTab({ raw, search, setSearch }: { raw: Record<string, string>; search: string; setSearch: (value: string) => void }) {
  const entries = useMemo(() => Object.entries(raw).filter(([key, value]) => `${key} ${value}`.toLowerCase().includes(search.toLowerCase())), [raw, search])
  return <div><label className="relative block"><ListFilter size={16} className="absolute top-1/2 left-3 -translate-y-1/2 text-slate-400" /><input value={search} onChange={(event) => setSearch(event.target.value)} placeholder="搜索变量名或值" className="field-input pl-10" /></label><div className="mt-4 overflow-hidden rounded-xl border border-slate-200"><div className="max-h-[36rem] overflow-auto"><table className="w-full text-left text-xs"><thead className="sticky top-0 bg-slate-100 text-slate-500"><tr><th className="px-4 py-3 font-medium">NUT 变量</th><th className="px-4 py-3 font-medium">当前值</th></tr></thead><tbody>{entries.map(([key, value]) => <tr key={key} className="border-t border-slate-200"><td className="px-4 py-3 font-mono text-slate-600">{key}</td><td className="break-all px-4 py-3 text-slate-800">{value || '—'}</td></tr>)}</tbody></table></div></div></div>
}

function DeviceCard({ device, selected, onClick }: { device: UpsMonitorDevice; selected: boolean; onClick: () => void }) {
  return <button type="button" onClick={onClick} className={`rounded-xl border p-4 text-left transition ${selected ? 'border-emerald-300 bg-emerald-50/60 shadow-sm' : 'border-slate-200 bg-white hover:border-slate-300'}`}><div className="flex items-start justify-between gap-3"><div className="min-w-0"><p className="truncate text-sm font-semibold text-slate-800">{deviceTitle(device)}</p><p className="mt-1 truncate text-xs text-slate-500">{device.source_name}</p></div><span className={`mt-1 size-2.5 shrink-0 rounded-full ${device.online ? isAbnormal(device) ? 'bg-amber-500' : 'bg-emerald-500' : 'bg-slate-300'}`} /></div><div className="mt-4 flex items-end justify-between"><div><p className="text-2xl font-semibold text-slate-900">{percent(device.charge_percent)}</p><p className="mt-1 text-[10px] text-slate-500">电池电量</p></div><span className="rounded-lg bg-slate-100 px-2 py-1 text-[10px] text-slate-600">{device.online ? device.status_flags.join(' ') || 'Unknown' : '离线'}</span></div></button>
}

function VariableGroup({ title, entries }: { title: string; entries: Array<[string, string]> }) {
  if (!entries.length) return null
  return <details open className="rounded-xl border border-slate-200 p-4"><summary className="cursor-pointer text-xs font-semibold text-slate-700">{title} · {entries.length}</summary><dl className="mt-3 space-y-2">{entries.map(([key, value]) => <div key={key} className="flex justify-between gap-4 text-xs"><dt className="min-w-0 truncate font-mono text-slate-500" title={key}>{key}</dt><dd className="shrink-0 text-right text-slate-800">{value || '—'}</dd></div>)}</dl></details>
}

function group(raw: Record<string, string>, prefixes: string[]) { return Object.entries(raw).filter(([key]) => prefixes.some((prefix) => key.startsWith(prefix))) }
function isAbnormal(device: UpsMonitorDevice) { return !device.online || device.status_flags.some((flag) => ['OB', 'LB', 'RB', 'BYPASS', 'OFF'].includes(flag)) }
function deviceTitle(device?: UpsMonitorDevice) { return device ? [device.manufacturer, device.model].filter(Boolean).join(' ') || device.description || 'UPS 设备' : '正在读取…' }
function percent(value?: number | null) { return value == null ? '—' : `${Math.round(value)}%` }
function unit(value: number | null | undefined, suffix: string) { return value == null ? '—' : `${Math.round(value * 10) / 10}${suffix}` }
function runtime(seconds?: number | null, capped = false) { if (capped) return '设备未提供'; if (seconds == null) return '—'; const minutes = Math.floor(seconds / 60); return minutes >= 60 ? `${Math.floor(minutes / 60)} 小时 ${minutes % 60} 分` : `${minutes} 分钟` }
function dateTime(value?: string | null) { return value ? new Intl.DateTimeFormat('zh-CN', { month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit', second: '2-digit' }).format(new Date(value)) : '尚未采集' }

function SummaryCard({ label, value, icon: Icon, tone }: { label: string; value: number; icon: typeof Server; tone: 'slate' | 'emerald' | 'cyan' | 'amber' | 'rose' }) { const colors = { slate: 'bg-slate-100 text-slate-600', emerald: 'bg-emerald-50 text-emerald-600', cyan: 'bg-cyan-50 text-cyan-700', amber: 'bg-amber-50 text-amber-600', rose: 'bg-rose-50 text-rose-600' }; return <article className="rounded-2xl border border-slate-200 bg-white p-5"><div className="flex items-center justify-between"><p className="text-xs font-medium text-slate-500">{label}</p><span className={`grid size-8 place-items-center rounded-lg ${colors[tone]}`}><Icon size={16} /></span></div><p className="mt-5 text-3xl font-semibold tracking-tight">{value}</p></article> }
function Metric({ icon: Icon, label, value, compact = false }: { icon: typeof Gauge; label: string; value: string; compact?: boolean }) { return <div className={`rounded-xl border border-slate-200 bg-slate-50 ${compact ? 'flex items-center justify-between p-4' : 'p-4'}`}><div className={compact ? 'flex items-center gap-2' : ''}><Icon size={16} className="text-emerald-600" /><p className={compact ? 'text-xs text-slate-500' : 'mt-4 text-xs text-slate-500'}>{label}</p></div><p className={`${compact ? 'text-lg' : 'mt-1 text-xl'} font-semibold text-slate-800`}>{value}</p></div> }
function StatusBadge({ device }: { device?: UpsMonitorDevice }) { if (!device?.online) return <span className="rounded-full bg-slate-100 px-3 py-1.5 text-xs text-slate-600">离线</span>; const abnormal = isAbnormal(device); return <span className={`rounded-full px-3 py-1.5 text-xs font-medium ${abnormal ? 'bg-amber-50 text-amber-700' : 'bg-emerald-50 text-emerald-700'}`}>{device.status_flags.join(' ') || '在线'}</span> }
function Notice({ text }: { text: string }) { return <div className="mt-6 flex items-center gap-3 rounded-xl border border-rose-200 bg-rose-50 p-4 text-sm text-rose-700"><AlertTriangle size={18} />{text}</div> }
