import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { AlertTriangle, CheckCircle2, Link2, LoaderCircle, RefreshCw, X } from 'lucide-react'
import * as Dialog from 'radix-ui/dialog'
import { useEffect, useState } from 'react'
import { toast } from 'sonner'
import {
  applyBindingConfig,
  bindingsQueryKey,
  createBinding,
  errorMessage,
  getOperation,
  previewBindingConfig,
} from '../lib/api.ts'
import type { Host, NutBindingRecord, NutServerRecord } from '../lib/types.ts'
import { ConfigPreviewPanel } from './ConfigPreviewPanel.tsx'

export function ClientConfigDialog({ host, servers, hosts, binding }: { host: Host; servers: NutServerRecord[]; hosts: Host[]; binding: NutBindingRecord | null }) {
  const [open, setOpen] = useState(false)
  const [draft, setDraft] = useState<NutBindingRecord | null>(binding)
  const [operationId, setOperationId] = useState<string | null>(null)
  const [selectedServerId, setSelectedServerId] = useState(binding?.server_id ?? '')
  const [confirmedSnapshot, setConfirmedSnapshot] = useState<string | null>(null)
  const queryClient = useQueryClient()
  const currentBinding = draft ?? binding
  const server = servers.find((candidate) => candidate.id === (currentBinding?.server_id ?? selectedServerId)) ?? null
  const serverHost = hosts.find((candidate) => candidate.id === server?.host_id) ?? null
  const activeServers = servers.filter((candidate) => candidate.enabled)
  const create = useMutation({
    mutationFn: () => createBinding(selectedServerId, host.id),
    onSuccess: (value) => { setDraft(value); void queryClient.invalidateQueries({ queryKey: bindingsQueryKey }) },
    onError: (error) => toast.error(errorMessage(error)),
  })
  const preview = useQuery({
    queryKey: ['bindings', currentBinding?.id, 'config-preview'],
    queryFn: () => previewBindingConfig(currentBinding!.id),
    enabled: open && currentBinding !== null && operationId === null,
    retry: false,
  })
  const previewSnapshot = previewKey(preview.data)
  const apply = useMutation({
    mutationFn: () => {
      const snapshots = []
      if (preview.data?.server.takeover_required && serverHost) snapshots.push({ host_id: serverHost.id, snapshot_hash: preview.data.server.snapshot_hash })
      if (preview.data?.client.takeover_required) snapshots.push({ host_id: host.id, snapshot_hash: preview.data.client.snapshot_hash })
      return applyBindingConfig(currentBinding!.id, snapshots)
    },
    onSuccess: ({ operation_id }) => setOperationId(operation_id),
    onError: (error) => toast.error(errorMessage(error)),
  })
  const operation = useQuery({
    queryKey: ['operations', operationId], queryFn: () => getOperation(operationId!), enabled: operationId !== null,
    refetchInterval: (query) => ['succeeded', 'failed'].includes(query.state.data?.state ?? '') ? false : 900,
  })
  useEffect(() => { if (operation.data?.state === 'succeeded') void queryClient.invalidateQueries({ queryKey: bindingsQueryKey }) }, [operation.data?.state, queryClient])

  return (
    <Dialog.Root open={open} onOpenChange={(nextOpen) => { setOpen(nextOpen); if (!nextOpen) setConfirmedSnapshot(null) }}>
      <Dialog.Trigger asChild><button type="button" disabled={!binding && activeServers.length === 0} className="inline-flex items-center gap-1.5 rounded-lg border border-violet-300/15 px-2.5 py-2 text-xs text-violet-600 hover:bg-violet-300/[0.05] disabled:cursor-not-allowed disabled:opacity-40"><Link2 size={14} />{binding?.apply_state === 'applied' ? 'Client 配置' : '绑定 Server'}</button></Dialog.Trigger>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-40 bg-black/65 backdrop-blur-sm" />
        <Dialog.Content className="dialog-content fixed top-1/2 left-1/2 z-50 max-h-[92vh] w-[calc(100%-2rem)] max-w-4xl -translate-x-1/2 -translate-y-1/2 overflow-y-auto rounded-2xl border border-slate-200 bg-white p-6 shadow-2xl focus:outline-none">
          <div className="flex items-start justify-between"><div><Dialog.Title className="text-lg font-semibold">NUT Client 配置</Dialog.Title><Dialog.Description className="mt-1.5 text-xs text-slate-500">明确选择并确认 Client 所监控的 NUT Server</Dialog.Description></div><Dialog.Close asChild><button type="button" aria-label="关闭" className="p-1.5 text-slate-500"><X size={18} /></button></Dialog.Close></div>
          <div className="mt-6">
            {!currentBinding && <div className="rounded-xl border border-slate-200 bg-slate-50 p-5">
              <label className="text-xs font-medium text-slate-600">选择 NUT Server
                <select value={selectedServerId} onChange={(event) => setSelectedServerId(event.target.value)} className="mt-2 w-full rounded-xl border border-slate-200 bg-slate-50 px-3 py-3 text-sm text-slate-800 outline-none focus:border-violet-300/40">
                  <option value="">请选择要绑定的 Server…</option>
                  {activeServers.map((candidate) => {
                    const candidateHost = hosts.find((item) => item.id === candidate.host_id)
                    return <option key={candidate.id} value={candidate.id}>{candidateHost?.name ?? candidate.ups_name} · {candidateHost?.address ?? '未知地址'}:{candidate.listen_port}</option>
                  })}
                </select>
              </label>
              {server && serverHost && <BindingCard client={host} server={server} serverHost={serverHost} />}
              <p className="mt-4 text-xs leading-5 text-slate-500">通用凭据：nwm / nwm</p>
              <button type="button" disabled={create.isPending || !selectedServerId} onClick={() => create.mutate()} className="mt-4 inline-flex w-full items-center justify-center gap-2 rounded-xl bg-violet-300 px-4 py-3 text-sm font-semibold text-violet-950 disabled:opacity-60">{create.isPending ? <LoaderCircle size={16} className="animate-spin" /> : <Link2 size={16} />}确认绑定到 {serverHost?.name ?? '所选 Server'}</button>
            </div>}
            {currentBinding && server && serverHost && <div className="mb-6"><BindingCard client={host} server={server} serverHost={serverHost} /></div>}
            {preview.isPending && currentBinding && !operationId && <Loading text="正在同时检查 Server 和 Client 配置…" />}
            {preview.isError && !operationId && <Failure title="无法生成双主机配置预览" detail={errorMessage(preview.error)} onRetry={() => void preview.refetch()} />}
            {preview.data && !operationId && <div className="space-y-6">
              <ConfigPreviewPanel title={`${serverHost?.name ?? 'Server'}：验证通用 Client 凭据`} preview={preview.data.server} />
              <ConfigPreviewPanel title={`${host.name}：netclient 与 secondary monitor`} preview={preview.data.client} />
              {(preview.data.server.takeover_required || preview.data.client.takeover_required) && <TakeoverConfirmation
                targets={[
                  preview.data.server.takeover_required ? serverHost?.name ?? 'Server' : null,
                  preview.data.client.takeover_required ? host.name : null,
                ].filter((value): value is string => value !== null)}
                allowed={(!preview.data.server.takeover_required || preview.data.server.takeover_allowed) && (!preview.data.client.takeover_required || preview.data.client.takeover_allowed)}
                reason={preview.data.server.takeover_block_reason ?? preview.data.client.takeover_block_reason}
                warning={preview.data.server.takeover_warning ?? preview.data.client.takeover_warning}
                confirmed={confirmedSnapshot === previewSnapshot}
                onConfirmed={(confirmed) => setConfirmedSnapshot(confirmed ? previewSnapshot : null)}
              />}
              <button type="button" disabled={apply.isPending || ((preview.data.server.takeover_required || preview.data.client.takeover_required) && (confirmedSnapshot !== previewSnapshot || !preview.data.server.takeover_allowed || !preview.data.client.takeover_allowed))} onClick={() => apply.mutate()} className="inline-flex w-full items-center justify-center gap-2 rounded-xl bg-violet-300 px-4 py-3 text-sm font-semibold text-violet-950 disabled:opacity-60">{apply.isPending ? <LoaderCircle size={16} className="animate-spin" /> : <Link2 size={16} />}{preview.data.server.takeover_required || preview.data.client.takeover_required ? '备份并接管保护链路' : `确认应用到 ${serverHost?.name ?? 'Server'} 与 ${host.name}`}</button>
            </div>}
            {operationId && !['succeeded', 'failed'].includes(operation.data?.state ?? '') && <Loading text="正在配置保护链路…" />}
            {operation.isError && <Failure title="无法读取配置任务" detail={errorMessage(operation.error)} onRetry={() => void operation.refetch()} />}
            {operation.data?.state === 'failed' && <Failure title={operation.data.error_code ?? 'Client 配置失败'} detail={operation.data.error_detail ?? '没有错误详情'} onRetry={() => setOperationId(null)} />}
            {operation.data?.state === 'succeeded' && <div className="rounded-xl border border-emerald-300/15 bg-emerald-300/[0.05] p-5 text-center"><CheckCircle2 size={28} className="mx-auto text-emerald-600" /><p className="mt-3 text-sm font-medium text-emerald-800">Client 已连接 Server，nut-monitor 和 upsc 验证完成</p>{bindingResultDetail(operation.data.result) && <pre className="mt-3 whitespace-pre-wrap break-all text-left text-[10px] leading-5 text-emerald-700/70">{bindingResultDetail(operation.data.result)}</pre>}<button type="button" onClick={() => setOperationId(null)} className="mt-4 text-xs text-emerald-600">查看当前配置</button></div>}
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}

function BindingCard({ client, server, serverHost }: { client: Host; server: NutServerRecord; serverHost: Host }) {
  const vendor = server.device.selectors.vendor ?? null
  const product = server.device.selectors.product ?? null
  return <div className="mt-4 rounded-xl border border-violet-300/15 bg-violet-300/[0.04] p-4">
    <p className="text-[10px] font-medium tracking-[0.14em] text-violet-600/80 uppercase">绑定关系确认</p>
    <div className="mt-3 grid items-center gap-3 text-xs sm:grid-cols-[1fr_auto_1fr_auto_1fr]">
      <Endpoint label="Client" name={client.name} detail={`${client.username}@${client.address}:${client.ssh_port}`} />
      <span className="text-center text-violet-600">→</span>
      <Endpoint label="Server" name={serverHost.name} detail={`${serverHost.address}:${server.listen_port}`} />
      <span className="text-center text-violet-600">→</span>
      <Endpoint label="UPS" name={[vendor, product].filter(Boolean).join(' ') || server.device.name} detail={server.ups_name} />
    </div>
  </div>
}

function Endpoint({ label, name, detail }: { label: string; name: string; detail: string }) {
  return <div className="min-w-0 rounded-lg border border-slate-200 bg-slate-100 p-3"><p className="text-[10px] text-slate-600 uppercase">{label}</p><p className="mt-1 truncate font-medium text-slate-800">{name}</p><p className="mt-1 truncate font-mono text-[10px] text-slate-500">{detail}</p></div>
}

function Loading({ text }: { text: string }) { return <div className="flex min-h-40 flex-col items-center justify-center rounded-xl border border-violet-300/10 bg-violet-300/[0.03]"><LoaderCircle size={22} className="animate-spin text-violet-600" /><p className="mt-3 text-sm text-slate-600">{text}</p></div> }
function Failure({ title, detail, onRetry }: { title: string; detail: string; onRetry: () => void }) { return <div className="rounded-xl border border-rose-400/15 bg-rose-400/[0.05] p-4"><p className="flex items-center gap-2 text-sm font-medium text-rose-700"><AlertTriangle size={17} />{title}</p><pre className="mt-3 max-h-52 overflow-auto whitespace-pre-wrap rounded-lg bg-slate-100 p-3 font-mono text-[11px] text-slate-600">{detail}</pre><button type="button" onClick={onRetry} className="mt-4 inline-flex items-center gap-1.5 text-xs text-rose-600"><RefreshCw size={13} />修复后重试</button></div> }

function TakeoverConfirmation({ targets, allowed, reason, warning, confirmed, onConfirmed }: { targets: string[]; allowed: boolean; reason: string | null; warning: string | null; confirmed: boolean; onConfirmed: (value: boolean) => void }) {
  return <div className={`rounded-xl border p-4 text-xs leading-5 ${allowed ? 'border-amber-300/25 bg-amber-300/[0.06] text-amber-800' : 'border-rose-400/25 bg-rose-400/[0.06] text-rose-800'}`}>
    <p className="flex items-center gap-2 font-medium"><AlertTriangle size={16} />需要接管：{targets.join('、')}</p>
    <p className="mt-2 opacity-75">双方会在任何写入前完成预检和备份。现有 NUT 用户、自定义 listener 和受管指令不会合并；任一主机失败时恢复双方配置。</p>
    {warning && <p className="mt-3 rounded-lg border border-amber-200/15 bg-amber-950/20 p-3 font-medium">{warning}</p>}
    {allowed ? <label className="mt-3 flex cursor-pointer items-start gap-2 rounded-lg border border-amber-200/15 bg-slate-100 p-3"><input type="checkbox" checked={confirmed} onChange={(event) => onConfirmed(event.target.checked)} className="mt-0.5 accent-amber-300" /><span>我确认已查看双方完整差异，并允许备份后替换冲突配置。</span></label> : <p className="mt-3 rounded-lg bg-rose-950/20 p-3">当前禁止接管：{reason ?? 'UPS 必须明确处于 OL 状态'}</p>}
  </div>
}

function previewKey(preview: import('../lib/types.ts').BindingConfigPreview | undefined) {
  return preview ? `${preview.server.snapshot_hash}:${preview.client.snapshot_hash}` : ''
}

function bindingResultDetail(result: unknown) {
  if (!result || typeof result !== 'object') return null
  const value = result as { server?: { backup_path?: unknown; revision_id?: unknown }; client?: { backup_path?: unknown; revision_id?: unknown } }
  if (typeof value.server?.backup_path !== 'string' || typeof value.client?.backup_path !== 'string') return null
  return `Server revision ${String(value.server.revision_id ?? '—')} · ${value.server.backup_path}\nClient revision ${String(value.client.revision_id ?? '—')} · ${value.client.backup_path}`
}
