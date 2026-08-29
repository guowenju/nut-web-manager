import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { AlertTriangle, CheckCircle2, FileCog, LoaderCircle, RefreshCw, X } from 'lucide-react'
import * as Dialog from 'radix-ui/dialog'
import { useEffect, useState } from 'react'
import { toast } from 'sonner'
import { applyServerConfig, errorMessage, getOperation, previewServerConfig, serversQueryKey } from '../lib/api.ts'
import type { NutServerRecord } from '../lib/types.ts'
import { ConfigPreviewPanel } from './ConfigPreviewPanel.tsx'

export function ServerConfigDialog({ server }: { server: NutServerRecord }) {
  const [open, setOpen] = useState(false)
  const [operationId, setOperationId] = useState<string | null>(null)
  const [confirmedSnapshot, setConfirmedSnapshot] = useState<string | null>(null)
  const queryClient = useQueryClient()
  const preview = useQuery({
    queryKey: ['servers', server.id, 'config-preview'],
    queryFn: () => previewServerConfig(server.id),
    enabled: open && operationId === null,
    retry: false,
  })
  const apply = useMutation({
    mutationFn: () => applyServerConfig(server.id, preview.data?.takeover_required ? [{ host_id: server.host_id, snapshot_hash: preview.data.snapshot_hash }] : []),
    onSuccess: ({ operation_id }) => setOperationId(operation_id),
    onError: (error) => toast.error(errorMessage(error)),
  })
  const operation = useQuery({
    queryKey: ['operations', operationId],
    queryFn: () => getOperation(operationId!),
    enabled: operationId !== null,
    refetchInterval: (query) => ['succeeded', 'failed'].includes(query.state.data?.state ?? '') ? false : 900,
  })

  useEffect(() => {
    if (operation.data?.state === 'succeeded') {
      void queryClient.invalidateQueries({ queryKey: serversQueryKey })
    }
  }, [operation.data?.state, queryClient])

  return (
    <Dialog.Root open={open} onOpenChange={(nextOpen) => { setOpen(nextOpen); if (!nextOpen) { setOperationId(null); setConfirmedSnapshot(null) } }}>
      <Dialog.Trigger asChild>
        <button type="button" className={`inline-flex items-center gap-1.5 rounded-lg border px-2.5 py-2 text-xs transition ${server.enabled ? 'border-emerald-300/15 text-emerald-600 hover:bg-emerald-300/[0.05]' : 'border-amber-300/15 text-amber-600 hover:bg-amber-300/[0.05]'}`}>
          <FileCog size={14} /> {server.enabled ? '应用配置' : '应用 Server'}
        </button>
      </Dialog.Trigger>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-40 bg-black/65 backdrop-blur-sm" />
        <Dialog.Content className="dialog-content fixed top-1/2 left-1/2 z-50 max-h-[92vh] w-[calc(100%-2rem)] max-w-4xl -translate-x-1/2 -translate-y-1/2 overflow-y-auto rounded-2xl border border-slate-200 bg-white p-6 shadow-2xl focus:outline-none">
          <Header title="应用 Server 配置" subtitle={`${server.ups_name} · ${server.device.driver} · ${server.listen_address}:${server.listen_port}`} />
          <div className="mt-6">
            {preview.isPending && !operationId && <Loading text="正在读取远端配置并检查所有权…" />}
            {preview.isError && !operationId && <ErrorBox title="无法生成配置预览" detail={errorMessage(preview.error)} onRetry={() => void preview.refetch()} />}
            {preview.data && !operationId && (
              <div>
                <div className={`mb-5 rounded-xl border p-3 text-xs leading-5 ${server.apply_state === 'applied' ? 'border-cyan-200 bg-cyan-50 text-slate-600' : 'border-amber-300 bg-amber-50 font-semibold text-amber-800'}`}>
                  当前关机策略：{server.apply_state === 'applied' ? shutdownPolicy(server) : `待重新应用：${shutdownPolicy(server)}`}
                </div>
                <ConfigPreviewPanel title="Server 文件变化（包含实际 NUT 凭据）" preview={preview.data} />
                {preview.data.takeover_required && (
                  <div className={`mt-5 rounded-xl border p-4 text-xs leading-5 ${preview.data.takeover_allowed ? 'border-amber-300/25 bg-amber-300/[0.06] text-amber-800' : 'border-rose-400/25 bg-rose-400/[0.06] text-rose-800'}`}>
                    <p className="flex items-center gap-2 font-medium"><AlertTriangle size={16} />检测到需要显式接管的 NUT 配置</p>
                    <p className="mt-2 opacity-75">接管前会备份当前文件，随后完整替换 NWM 管理的配置。原有 NUT 用户、自定义 listener 和受管指令不会合并。</p>
                    {preview.data.takeover_warning && <p className="mt-3 rounded-lg border border-amber-200/15 bg-amber-950/20 p-3 font-medium">{preview.data.takeover_warning}</p>}
                    {preview.data.takeover_allowed ? (
                      <label className="mt-3 flex cursor-pointer items-start gap-2 rounded-lg border border-amber-200/15 bg-slate-100 p-3">
                        <input type="checkbox" checked={confirmedSnapshot === preview.data.snapshot_hash} onChange={(event) => setConfirmedSnapshot(event.target.checked ? preview.data.snapshot_hash : null)} className="mt-0.5 accent-amber-300" />
                        <span>我确认已查看完整差异，并允许备份后替换这些 NUT 配置。</span>
                      </label>
                    ) : <p className="mt-3 rounded-lg bg-rose-950/20 p-3">当前禁止接管：{preview.data.takeover_block_reason ?? 'UPS 必须明确处于 OL 状态'}</p>}
                  </div>
                )}
                <div className="mt-5 rounded-xl border border-amber-300/15 bg-amber-300/[0.04] p-3 text-xs leading-5 text-slate-500">
                  将开放 NUT IPv4 TCP 3493 listener，但不会修改主机防火墙。应用失败时恢复原文件并尝试恢复原服务状态。
                </div>
                <button type="button" disabled={apply.isPending || (preview.data.takeover_required && (!preview.data.takeover_allowed || confirmedSnapshot !== preview.data.snapshot_hash))} onClick={() => apply.mutate()} className="mt-4 inline-flex w-full items-center justify-center gap-2 rounded-xl bg-cyan-300 px-4 py-3 text-sm font-semibold text-cyan-950 hover:bg-cyan-200 disabled:opacity-60">
                  {apply.isPending ? <LoaderCircle size={17} className="animate-spin" /> : <FileCog size={17} />} {preview.data.takeover_required ? '备份并接管' : '确认应用并验证'}
                </button>
              </div>
            )}
            {operationId && !['succeeded', 'failed'].includes(operation.data?.state ?? '') && <Loading text="正在应用配置并验证…" />}
            {operation.isError && <ErrorBox title="无法读取配置任务" detail={errorMessage(operation.error)} onRetry={() => void operation.refetch()} />}
            {operation.data?.state === 'failed' && <ErrorBox title={operation.data.error_code ?? '配置失败'} detail={operation.data.error_detail ?? '没有错误详情'} onRetry={() => setOperationId(null)} />}
            {operation.data?.state === 'succeeded' && (
              <div className="rounded-xl border border-emerald-300/15 bg-emerald-300/[0.05] p-5 text-center">
                <CheckCircle2 size={28} className="mx-auto text-emerald-600" />
                <p className="mt-3 text-sm font-medium text-emerald-800">Server 配置及本地 NUT 验证完成</p>
                {operationResultDetail(operation.data.result) && <p className="mt-2 break-all text-[11px] leading-5 text-emerald-700/70">{operationResultDetail(operation.data.result)}</p>}
                {operationWarning(operation.data.result) && <p className="mt-3 text-xs leading-5 text-amber-700">{operationWarning(operation.data.result)}</p>}
                <button type="button" onClick={() => { setOperationId(null); void preview.refetch() }} className="mt-4 text-xs text-emerald-600">重新检查配置</button>
              </div>
            )}
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}

function shutdownPolicy(server: NutServerRecord) {
  const trigger = server.shutdown.trigger_mode === 'battery_level'
    ? `电池电量低于 ${server.shutdown.battery_level_percent}%`
    : `市电断电持续 ${server.shutdown.on_battery_seconds} 秒，或电量低于 ${server.shutdown.battery_level_percent}%`
  return `${trigger}；${server.shutdown.powerdown_enabled ? '关机后切断 UPS 全部输出' : 'UPS 输出保持开启'}`
}

function Header({ title, subtitle }: { title: string; subtitle: string }) {
  return <div className="flex items-start justify-between gap-4"><div><Dialog.Title className="text-lg font-semibold text-slate-900">{title}</Dialog.Title><Dialog.Description className="mt-1.5 text-xs text-slate-500">{subtitle}</Dialog.Description></div><Dialog.Close asChild><button type="button" aria-label="关闭" className="rounded-lg p-1.5 text-slate-500 hover:bg-slate-100"><X size={18} /></button></Dialog.Close></div>
}

function Loading({ text }: { text: string }) {
  return <div className="flex min-h-40 flex-col items-center justify-center rounded-xl border border-cyan-300/10 bg-cyan-300/[0.03]"><LoaderCircle size={22} className="animate-spin text-cyan-700" /><p className="mt-3 text-sm text-slate-600">{text}</p></div>
}

function ErrorBox({ title, detail, onRetry }: { title: string; detail: string; onRetry: () => void }) {
  return <div className="rounded-xl border border-rose-400/15 bg-rose-400/[0.05] p-4"><p className="flex items-center gap-2 text-sm font-medium text-rose-700"><AlertTriangle size={17} />{title}</p><pre className="mt-3 max-h-52 overflow-auto whitespace-pre-wrap rounded-lg bg-slate-100 p-3 font-mono text-[11px] text-slate-600">{detail}</pre><button type="button" onClick={onRetry} className="mt-4 inline-flex items-center gap-1.5 text-xs text-rose-600"><RefreshCw size={13} />修复后重试</button></div>
}

function operationWarning(result: unknown) {
  if (!result || typeof result !== 'object') return null
  const warning = (result as { warning?: unknown }).warning
  return typeof warning === 'string' ? warning : null
}

function operationResultDetail(result: unknown) {
  if (!result || typeof result !== 'object') return null
  const value = result as { backup_path?: unknown; takeover?: unknown; revision_id?: unknown }
  if (typeof value.backup_path !== 'string') return null
  const revision = typeof value.revision_id === 'string' ? ` · Revision：${value.revision_id}` : ''
  return `${value.takeover === true ? '接管完成' : '配置完成'}${revision} · 备份：${value.backup_path}`
}
