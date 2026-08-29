import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  AlertTriangle,
  Check,
  CheckCircle2,
  Copy,
  LoaderCircle,
  PackageCheck,
  PackagePlus,
  RefreshCw,
  Power,
  X,
} from 'lucide-react'
import * as Dialog from 'radix-ui/dialog'
import { useEffect, useMemo, useRef, useState } from 'react'
import { toast } from 'sonner'
import {
  ApiError,
  bindingsQueryKey,
  deactivateNut,
  errorMessage,
  getNutInstallStatus,
  getOperation,
  hostsQueryKey,
  installNut,
  serversQueryKey,
} from '../lib/api.ts'
import type { Host } from '../lib/types.ts'

export function NutInstallDialog({ host }: { host: Host }) {
  const [open, setOpen] = useState(false)
  const [copied, setCopied] = useState(false)
  const [operationId, setOperationId] = useState<string | null>(null)
  const [operationKind, setOperationKind] = useState<'install' | 'deactivate'>('install')
  const [confirmDeactivate, setConfirmDeactivate] = useState(false)
  const handledOperation = useRef<string | null>(null)
  const queryClient = useQueryClient()
  const statusKey = useMemo(() => ['hosts', host.id, 'nut-install'] as const, [host.id])
  const status = useQuery({
    queryKey: statusKey,
    queryFn: () => getNutInstallStatus(host.id),
    enabled: open && operationId === null,
    retry: false,
  })
  const operation = useQuery({
    queryKey: ['operations', operationId],
    queryFn: () => getOperation(operationId!),
    enabled: operationId !== null,
    refetchInterval: (query) => {
      const state = query.state.data?.state
      return state === 'succeeded' || state === 'failed' ? false : 1000
    },
  })
  const install = useMutation({
    mutationFn: () => installNut(host.id),
    onSuccess: ({ operation_id }) => {
      setOperationKind('install')
      handledOperation.current = null
      setOperationId(operation_id)
    },
    onError: (error) => toast.error(errorMessage(error)),
  })
  const deactivate = useMutation({
    mutationFn: () => deactivateNut(host.id),
    onSuccess: ({ operation_id }) => {
      setOperationKind('deactivate')
      setConfirmDeactivate(false)
      handledOperation.current = null
      setOperationId(operation_id)
    },
    onError: (error) => toast.error(errorMessage(error)),
  })

  useEffect(() => {
    const current = operation.data
    if (!current || (current.state !== 'succeeded' && current.state !== 'failed')) return
    if (handledOperation.current === current.id) return
    handledOperation.current = current.id
    if (current.state === 'succeeded') {
      toast.success(operationKind === 'install' ? `${host.name} 的 NUT 软件包已就绪` : `${host.name} 已停用 NUT`)
      void queryClient.invalidateQueries({ queryKey: hostsQueryKey })
      void queryClient.invalidateQueries({ queryKey: statusKey })
      void queryClient.invalidateQueries({ queryKey: serversQueryKey })
      void queryClient.invalidateQueries({ queryKey: bindingsQueryKey })
    } else {
      toast.error(current.error_code ?? (operationKind === 'install' ? '安装失败' : '停用失败'))
    }
  }, [host.name, operation.data, operationKind, queryClient, statusKey])

  async function copyCommand(command: string) {
    try {
      if (navigator.clipboard && window.isSecureContext) {
        await navigator.clipboard.writeText(command)
      } else if (!fallbackCopy(command)) {
        throw new Error('clipboard unavailable')
      }
      setCopied(true)
      window.setTimeout(() => setCopied(false), 1800)
      toast.success('安装命令已复制')
    } catch {
      toast.error('复制失败，请手动选择命令')
    }
  }

  function resetAndCheck() {
    setOperationId(null)
    handledOperation.current = null
    void status.refetch()
  }

  const operationPending = operationId !== null
    && !operation.isError
    && operation.data?.state !== 'succeeded'
    && operation.data?.state !== 'failed'

  return (
    <Dialog.Root
      open={open}
      onOpenChange={setOpen}
    >
      <Dialog.Trigger asChild>
        <button
          type="button"
          className="inline-flex items-center gap-1.5 rounded-lg border border-slate-200 px-2.5 py-2 text-xs text-slate-600 transition hover:border-cyan-300 hover:bg-cyan-50 hover:text-cyan-700"
        >
          <PackagePlus size={14} /> NUT 软件
        </button>
      </Dialog.Trigger>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-40 bg-black/65 backdrop-blur-sm" />
        <Dialog.Content className="dialog-content fixed top-1/2 left-1/2 z-50 max-h-[90vh] w-[calc(100%-2rem)] max-w-xl -translate-x-1/2 -translate-y-1/2 overflow-y-auto rounded-2xl border border-slate-200 bg-white p-6 shadow-2xl shadow-black/50 focus:outline-none">
          <div className="flex items-start justify-between gap-4">
            <div>
              <Dialog.Title className="text-lg font-semibold text-slate-900">NUT 软件</Dialog.Title>
              <Dialog.Description className="mt-1.5 text-xs text-slate-500">
                {host.name} · {host.role === 'server' ? 'Server' : 'Client'} 角色
              </Dialog.Description>
            </div>
            <Dialog.Close asChild>
              <button type="button" aria-label="关闭" className="rounded-lg p-1.5 text-slate-500 hover:bg-slate-100 hover:text-slate-800">
                <X size={18} />
              </button>
            </Dialog.Close>
          </div>

          <div className="mt-6">
            {(status.isPending || install.isPending) && !operationId && (
              <Loading label={install.isPending ? '正在创建安装任务…' : '正在检查软件包状态…'} />
            )}

            {status.isError && !operationId && (
              <ErrorPanel error={status.error} />
            )}

            {status.data?.installed && !operationId && (
              <div>
                <div className="rounded-xl border border-emerald-300/15 bg-emerald-300/[0.05] p-5">
                  <div className="flex items-center gap-2 text-sm font-medium text-emerald-700">
                    <PackageCheck size={19} /> {status.data.package} 已安装
                  </div>
                  <dl className="mt-4 grid grid-cols-[6rem_1fr] gap-2 text-xs">
                    <dt className="text-slate-500">版本</dt>
                    <dd className="font-mono text-slate-700">{status.data.version}</dd>
                    <dt className="text-slate-500">平台</dt>
                    <dd className="text-slate-700">{status.data.platform.hostname}</dd>
                  </dl>
                </div>
                {!confirmDeactivate ? (
                  <button type="button" onClick={() => setConfirmDeactivate(true)} className="mt-4 inline-flex w-full items-center justify-center gap-2 rounded-xl border border-rose-200 px-4 py-3 text-sm font-medium text-rose-600 hover:bg-rose-50">
                    <Power size={16} /> 停用并删除 NWM 配置
                  </button>
                ) : (
                  <div className="mt-4 rounded-xl border border-rose-200 bg-rose-50 p-4 text-xs text-rose-700">
                    <p>停止此主机的远端 NUT 服务，并删除其 NWM 配置记录。远端软件包、配置文件和 SSH 公钥保留。</p>
                    {host.role === 'server' && <p className="mt-2">关联的 Binding 记录会删除，其它 Client 主机不会自动停用。</p>}
                    <div className="mt-4 flex justify-end gap-2">
                      <button type="button" onClick={() => setConfirmDeactivate(false)} className="rounded-lg px-3 py-2 text-slate-600">取消</button>
                      <button type="button" disabled={deactivate.isPending} onClick={() => deactivate.mutate()} className="rounded-lg bg-rose-600 px-3 py-2 font-medium text-white disabled:opacity-50">确认停用</button>
                    </div>
                  </div>
                )}
              </div>
            )}

            {status.data && !status.data.installed && !operationId && (
              <div>
                <div className="rounded-xl border border-amber-300/15 bg-amber-300/[0.05] p-4">
                  <div className="flex items-center gap-2 text-sm font-medium text-amber-700">
                    <AlertTriangle size={17} /> 尚未安装 {status.data.package}
                  </div>
                  <p className="mt-2 text-xs leading-5 text-slate-600">
                    可以复制命令在主机终端手动执行，或由 NWM 通过 SSH 自动安装。
                  </p>
                </div>

                <div className="mt-4">
                  <p className="mb-2 text-xs font-medium text-slate-600">手动安装命令</p>
                  <div className="flex gap-2 rounded-xl border border-slate-200 bg-slate-50 p-2">
                    <code className="min-w-0 flex-1 overflow-x-auto px-2 py-1.5 font-mono text-xs whitespace-nowrap text-slate-700">
                      {status.data.install_command}
                    </code>
                    <button
                      type="button"
                      onClick={() => void copyCommand(status.data.install_command)}
                      className="inline-flex shrink-0 items-center gap-1.5 rounded-lg border border-slate-200 px-2.5 text-xs text-slate-600 hover:bg-slate-100 hover:text-slate-800"
                    >
                      {copied ? <Check size={14} className="text-emerald-600" /> : <Copy size={14} />}
                      {copied ? '已复制' : '复制'}
                    </button>
                  </div>
                </div>

                <button
                  type="button"
                  disabled={install.isPending}
                  onClick={() => install.mutate()}
                  className="mt-5 inline-flex w-full items-center justify-center gap-2 rounded-xl bg-cyan-300 px-4 py-3 text-sm font-semibold text-cyan-950 hover:bg-cyan-200 disabled:opacity-60"
                >
                  {install.isPending ? <LoaderCircle size={17} className="animate-spin" /> : <PackagePlus size={17} />}
                  自动安装 {status.data.package}
                </button>
              </div>
            )}

            {operationId && operationPending && (
              <div className="rounded-xl border border-cyan-300/12 bg-cyan-300/[0.04] p-5">
                <div className="flex items-center gap-3">
                  <LoaderCircle size={19} className="animate-spin text-cyan-700" />
                  <div>
                    <p className="text-sm font-medium text-slate-800">{operationKind === 'install' ? '正在安装 NUT' : '正在停用 NUT'}</p>
                  </div>
                </div>
              </div>
            )}

            {operationId && operation.isError && (
              <div className="rounded-xl border border-rose-400/15 bg-rose-400/[0.05] p-4">
                <div className="flex items-center gap-2 text-sm font-medium text-rose-700">
                  <AlertTriangle size={17} /> 无法读取{operationKind === 'install' ? '安装' : '停用'}任务
                </div>
                <p className="mt-3 text-xs text-slate-600">{errorMessage(operation.error)}</p>
                <button
                  type="button"
                  onClick={() => void operation.refetch()}
                  className="mt-4 inline-flex items-center gap-1.5 text-xs font-medium text-rose-600 hover:text-rose-700"
                >
                  <RefreshCw size={13} /> 重试查询
                </button>
              </div>
            )}

            {operation.data?.state === 'succeeded' && (
              <div className="rounded-xl border border-emerald-300/15 bg-emerald-300/[0.05] p-5 text-center">
                <CheckCircle2 size={28} className="mx-auto text-emerald-600" />
                <p className="mt-3 text-sm font-medium text-emerald-800">{operationKind === 'install' ? '安装及验证完成' : 'NUT 已停用，本地配置记录已删除'}</p>
                <button type="button" onClick={operationKind === 'deactivate' ? () => setOpen(false) : resetAndCheck} className="mt-4 text-xs font-medium text-emerald-600 hover:text-emerald-700">
                  {operationKind === 'deactivate' ? '关闭' : '查看已安装版本'}
                </button>
              </div>
            )}

            {operation.data?.state === 'failed' && (
              <div className="rounded-xl border border-rose-400/15 bg-rose-400/[0.05] p-4">
                <div className="flex items-center gap-2 text-sm font-medium text-rose-700">
                  <AlertTriangle size={17} /> {operation.data.error_code ?? (operationKind === 'install' ? '安装失败' : '停用失败')}
                </div>
                <pre className="mt-3 max-h-48 overflow-auto whitespace-pre-wrap rounded-lg bg-slate-100 p-3 font-mono text-[11px] leading-5 text-slate-600">
                  {operation.data.error_detail ?? '目标主机未返回错误详情'}
                </pre>
                <button type="button" onClick={resetAndCheck} className="mt-4 inline-flex items-center gap-1.5 text-xs font-medium text-rose-600 hover:text-rose-700">
                  <RefreshCw size={13} /> 修复后重新检测
                </button>
              </div>
            )}
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}

function Loading({ label }: { label: string }) {
  return (
    <div className="flex min-h-36 flex-col items-center justify-center rounded-xl border border-slate-200 bg-slate-50">
      <LoaderCircle size={22} className="animate-spin text-cyan-700" />
      <p className="mt-3 text-sm text-slate-600">{label}</p>
    </div>
  )
}

function ErrorPanel({ error }: { error: unknown }) {
  const code = error instanceof ApiError ? error.code : 'StatusUnavailable'
  const advice: Record<string, string> = {
    HostKeyConfirmationRequired: '请先完成 SSH Host Key 确认。',
    HostKeyChanged: 'Host Key 已变化，请先重新核对指纹。',
    UnsupportedPlatform: '自动安装仅支持 Debian 13、PVE 9 和 PBS 4。',
    SshUnavailable: '请确认 NWM 公钥已添加到目标 root 用户。',
  }
  return (
    <div className="rounded-xl border border-rose-400/15 bg-rose-400/[0.05] p-4">
      <div className="flex items-center gap-2 text-sm font-medium text-rose-700"><AlertTriangle size={17} /> 无法检查安装状态</div>
      <p className="mt-3 text-xs leading-5 text-slate-600">{errorMessage(error)}</p>
      {advice[code] && <p className="mt-2 text-xs text-slate-500">{advice[code]}</p>}
    </div>
  )
}

function fallbackCopy(value: string): boolean {
  const textarea = document.createElement('textarea')
  textarea.value = value
  textarea.style.position = 'fixed'
  textarea.style.opacity = '0'
  document.body.appendChild(textarea)
  textarea.select()
  const copied = document.execCommand('copy')
  textarea.remove()
  return copied
}
