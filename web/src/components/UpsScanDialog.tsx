import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  AlertTriangle,
  Check,
  LoaderCircle,
  RefreshCw,
  ScanSearch,
  Usb,
  X,
} from 'lucide-react'
import * as Dialog from 'radix-ui/dialog'
import { useMemo, useState } from 'react'
import { toast } from 'sonner'
import {
  errorMessage,
  getOperation,
  scanUsbUps,
  selectServerDevice,
  serversQueryKey,
} from '../lib/api.ts'
import type { Host, UsbScanCandidate, UsbScanResult } from '../lib/types.ts'

export function UpsScanDialog({ host }: { host: Host }) {
  const [open, setOpen] = useState(false)
  const [operationId, setOperationId] = useState<string | null>(null)
  const [selectedIndex, setSelectedIndex] = useState<number | null>(null)
  const queryClient = useQueryClient()
  const scan = useMutation({
    mutationFn: () => scanUsbUps(host.id),
    onSuccess: ({ operation_id }) => {
      setSelectedIndex(null)
      setOperationId(operation_id)
    },
    onError: (error) => toast.error(errorMessage(error)),
  })
  const operation = useQuery({
    queryKey: ['operations', operationId],
    queryFn: () => getOperation(operationId!),
    enabled: operationId !== null,
    refetchInterval: (query) => {
      const state = query.state.data?.state
      return state === 'succeeded' || state === 'failed' ? false : 800
    },
  })
  const saveSelection = useMutation({
    mutationFn: (candidate: UsbScanCandidate) => selectServerDevice(host.id, candidate),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: serversQueryKey })
      toast.success('UPS 设备已保存，可以预览并应用 Server 配置')
      setOpen(false)
    },
    onError: (error) => toast.error(errorMessage(error)),
  })
  const result = useMemo(() => scanResult(operation.data?.result), [operation.data?.result])
  const selected = selectedIndex === null
    ? null
    : result?.candidates.find((candidate) => candidate.index === selectedIndex) ?? null

  function startScan() {
    scan.mutate()
  }

  const pending = operationId !== null
    && !operation.isError
    && operation.data?.state !== 'succeeded'
    && operation.data?.state !== 'failed'

  return (
    <Dialog.Root open={open} onOpenChange={setOpen}>
      <Dialog.Trigger asChild>
        <button
          type="button"
          className="inline-flex items-center gap-1.5 rounded-lg border border-slate-200 px-2.5 py-2 text-xs text-slate-600 transition hover:border-cyan-300 hover:bg-cyan-50 hover:text-cyan-700"
        >
          <Usb size={14} /> 扫描 UPS
        </button>
      </Dialog.Trigger>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-40 bg-black/65 backdrop-blur-sm" />
        <Dialog.Content className="dialog-content fixed top-1/2 left-1/2 z-50 max-h-[90vh] w-[calc(100%-2rem)] max-w-2xl -translate-x-1/2 -translate-y-1/2 overflow-y-auto rounded-2xl border border-slate-200 bg-white p-6 shadow-2xl shadow-black/50 focus:outline-none">
          <div className="flex items-start justify-between gap-4">
            <div>
              <Dialog.Title className="text-lg font-semibold text-slate-900">扫描 USB UPS</Dialog.Title>
              <Dialog.Description className="mt-1.5 text-xs text-slate-500">
                {host.name} · 通过 nut-scanner 读取本机 USB 设备
              </Dialog.Description>
            </div>
            <Dialog.Close asChild>
              <button type="button" aria-label="关闭" className="rounded-lg p-1.5 text-slate-500 hover:bg-slate-100 hover:text-slate-800">
                <X size={18} />
              </button>
            </Dialog.Close>
          </div>

          <div className="mt-6">
            {!operationId && (
              <div className="rounded-xl border border-slate-200 bg-slate-50 p-5">
                <div className="flex items-center gap-2 text-sm font-medium text-slate-800">
                  <ScanSearch size={18} className="text-cyan-700" /> 查找可用的 USB UPS
                </div>
                <button
                  type="button"
                  disabled={scan.isPending}
                  onClick={startScan}
                  className="mt-5 inline-flex w-full items-center justify-center gap-2 rounded-xl bg-cyan-300 px-4 py-3 text-sm font-semibold text-cyan-950 hover:bg-cyan-200 disabled:opacity-60"
                >
                  {scan.isPending ? <LoaderCircle size={17} className="animate-spin" /> : <ScanSearch size={17} />}
                  开始扫描
                </button>
              </div>
            )}

            {pending && (
              <div className="flex min-h-44 flex-col items-center justify-center rounded-xl border border-cyan-300/12 bg-cyan-300/[0.04]">
                <LoaderCircle size={23} className="animate-spin text-cyan-700" />
                <p className="mt-3 text-sm text-slate-700">正在扫描 USB 总线…</p>
                <p className="mt-1 text-xs text-slate-500">部分设备探测可能需要几十秒</p>
              </div>
            )}

            {operation.isError && operationId && (
              <Failure title="无法读取扫描任务" detail={errorMessage(operation.error)} onRetry={() => void operation.refetch()} />
            )}

            {operation.data?.state === 'failed' && (
              <Failure
                title={failureTitle(operation.data.error_code)}
                detail={operation.data.error_detail ?? '目标主机未返回错误详情'}
                onRetry={() => {
                  setOperationId(null)
                  setSelectedIndex(null)
                }}
              />
            )}

            {operation.data?.state === 'succeeded' && result && result.candidates.length === 0 && (
              <div className="rounded-xl border border-amber-300/15 bg-amber-300/[0.05] p-5 text-center">
                <Usb size={26} className="mx-auto text-amber-600" />
                <p className="mt-3 text-sm font-medium text-amber-800">未发现兼容的 USB UPS</p>
                <p className="mt-2 text-xs leading-5 text-slate-500">请检查 USB 连接和设备权限，然后重新扫描。</p>
                <RescanButton onClick={startScan} pending={scan.isPending} />
              </div>
            )}

            {operation.data?.state === 'succeeded' && result && result.candidates.length > 0 && (
              <div>
                <div className="flex items-center justify-between gap-3">
                  <div>
                    <p className="text-sm font-medium text-slate-800">发现 {result.candidates.length} 个候选设备</p>
                    <p className="mt-1 text-xs text-slate-500">请选择当前 Server 要使用的一台 UPS。</p>
                  </div>
                  <RescanButton onClick={startScan} pending={scan.isPending} compact />
                </div>
                <div className="mt-4 space-y-2.5">
                  {result.candidates.map((candidate) => (
                    <CandidateCard
                      key={candidate.index}
                      candidate={candidate}
                      selected={selectedIndex === candidate.index}
                      onSelect={() => setSelectedIndex(candidate.index)}
                    />
                  ))}
                </div>
                {selected && (
                  <div className="mt-4 rounded-xl border border-emerald-300/15 bg-emerald-300/[0.05] p-4 text-xs text-emerald-800">
                    <span className="inline-flex items-center gap-1.5 font-medium"><Check size={14} /> 已选择 {displayName(selected)}</span>
                    <p className="mt-1.5 leading-5 text-slate-500">保存后只创建 NWM 配置草稿，不会立即写入远端或重启 NUT 服务。</p>
                    <button
                      type="button"
                      disabled={saveSelection.isPending}
                      onClick={() => saveSelection.mutate(selected)}
                      className="mt-3 inline-flex w-full items-center justify-center gap-2 rounded-lg bg-emerald-300 px-3 py-2.5 text-xs font-semibold text-emerald-950 hover:bg-emerald-200 disabled:opacity-60"
                    >
                      {saveSelection.isPending ? <LoaderCircle size={14} className="animate-spin" /> : <Check size={14} />}
                      保存设备并进入配置阶段
                    </button>
                  </div>
                )}
              </div>
            )}

            {operation.data?.state === 'succeeded' && !result && (
              <Failure title="扫描结果格式无效" detail="后台任务已完成，但没有返回可识别的候选设备数据。" onRetry={() => setOperationId(null)} />
            )}
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}

function CandidateCard({ candidate, selected, onSelect }: { candidate: UsbScanCandidate; selected: boolean; onSelect: () => void }) {
  return (
    <button
      type="button"
      onClick={onSelect}
      className={`flex w-full items-start gap-3 rounded-xl border p-4 text-left transition ${selected ? 'border-cyan-300/35 bg-cyan-300/[0.07]' : 'border-slate-200 bg-slate-50 hover:border-slate-300'}`}
    >
      <span className={`mt-0.5 grid size-5 shrink-0 place-items-center rounded-full border ${selected ? 'border-cyan-300 bg-cyan-300 text-cyan-950' : 'border-slate-600'}`}>
        {selected && <Check size={12} strokeWidth={3} />}
      </span>
      <span className="min-w-0 flex-1">
        <span className="block truncate text-sm font-medium text-slate-800">{displayName(candidate)}</span>
        <span className="mt-1.5 block font-mono text-[11px] text-slate-500">{candidate.driver} · port={candidate.port}</span>
        <span className="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-[10px] text-slate-600">
          {(candidate.vendor_id || candidate.product_id) && <span>USB {candidate.vendor_id ?? '????'}:{candidate.product_id ?? '????'}</span>}
          {candidate.serial && <span>序列号 {candidate.serial}</span>}
          {candidate.bus && <span>总线 {candidate.bus}{candidate.device ? `/${candidate.device}` : ''}</span>}
        </span>
      </span>
    </button>
  )
}

function Failure({ title, detail, onRetry }: { title: string; detail: string; onRetry: () => void }) {
  return (
    <div className="rounded-xl border border-rose-400/15 bg-rose-400/[0.05] p-4">
      <div className="flex items-center gap-2 text-sm font-medium text-rose-700"><AlertTriangle size={17} /> {title}</div>
      <pre className="mt-3 max-h-48 overflow-auto whitespace-pre-wrap rounded-lg bg-slate-100 p-3 font-mono text-[11px] leading-5 text-slate-600">{detail}</pre>
      <button type="button" onClick={onRetry} className="mt-4 inline-flex items-center gap-1.5 text-xs font-medium text-rose-600 hover:text-rose-700">
        <RefreshCw size={13} /> 修复后重试
      </button>
    </div>
  )
}

function RescanButton({ onClick, pending, compact = false }: { onClick: () => void; pending: boolean; compact?: boolean }) {
  return (
    <button
      type="button"
      disabled={pending}
      onClick={onClick}
      className={`${compact ? '' : 'mt-4'} inline-flex items-center gap-1.5 rounded-lg border border-slate-200 px-3 py-2 text-xs text-slate-600 hover:bg-slate-100 hover:text-slate-800 disabled:opacity-60`}
    >
      {pending ? <LoaderCircle size={13} className="animate-spin" /> : <RefreshCw size={13} />} 重新扫描
    </button>
  )
}

function scanResult(value: unknown): UsbScanResult | null {
  if (!value || typeof value !== 'object') return null
  const candidate = value as Partial<UsbScanResult>
  return Array.isArray(candidate.candidates) && typeof candidate.scanned_at === 'string' ? candidate as UsbScanResult : null
}

function displayName(candidate: UsbScanCandidate) {
  return [candidate.vendor, candidate.product].filter(Boolean).join(' ') || `USB UPS ${candidate.index + 1}`
}

function failureTitle(code: string | null) {
  const labels: Record<string, string> = {
    NutServerNotInstalled: '请先安装 NUT',
    ScannerUnavailable: 'nut-scanner 不可用',
    UsbScanUnavailable: '当前 NUT 不支持 USB 扫描',
    UsbScanTimedOut: 'USB 扫描超时',
    HostKeyConfirmationRequired: '请先确认 SSH Host Key',
    HostKeyChanged: 'SSH Host Key 已变化',
  }
  return labels[code ?? ''] ?? code ?? 'USB 扫描失败'
}
