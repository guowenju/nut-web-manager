import { useMutation, useQueryClient } from '@tanstack/react-query'
import {
  AlertTriangle,
  CheckCircle2,
  Fingerprint,
  LoaderCircle,
  RefreshCw,
  ShieldCheck,
  Terminal,
  X,
} from 'lucide-react'
import * as Dialog from 'radix-ui/dialog'
import { useState } from 'react'
import { toast } from 'sonner'
import {
  detectHostEnvironment,
  errorMessage,
  hostsQueryKey,
  testSsh,
  trustSshHostKey,
} from '../lib/api.ts'
import type { EnvironmentReport, Host, SshTestReport } from '../lib/types.ts'

export function SshConnectionDialog({ host }: { host: Host }) {
  const [open, setOpen] = useState(false)
  const [report, setReport] = useState<SshTestReport | null>(null)
  const [environment, setEnvironment] = useState<EnvironmentReport | null>(null)
  const queryClient = useQueryClient()

  const environmentMutation = useMutation({
    mutationFn: () => detectHostEnvironment(host.id),
    onSuccess: (result) => {
      setEnvironment(result)
      void queryClient.invalidateQueries({ queryKey: hostsQueryKey })
      if (result.supported) toast.success('环境检测完成')
    },
    onError: (error) => toast.error(errorMessage(error)),
  })
  const testMutation = useMutation({
    mutationFn: () => testSsh(host.id),
    onSuccess: (result) => {
      setReport(result)
      setEnvironment(null)
      if (result.connected) environmentMutation.mutate()
    },
  })
  const trustMutation = useMutation({
    mutationFn: (fingerprint: string) => trustSshHostKey(host.id, fingerprint),
    onSuccess: () => {
      setReport(null)
      testMutation.mutate()
    },
    onError: (error) => toast.error(errorMessage(error)),
  })

  function scan() {
    setReport(null)
    setEnvironment(null)
    testMutation.reset()
    environmentMutation.reset()
    testMutation.mutate()
  }

  const pending = testMutation.isPending || trustMutation.isPending || environmentMutation.isPending

  return (
    <Dialog.Root
      open={open}
      onOpenChange={(nextOpen) => {
        setOpen(nextOpen)
        if (nextOpen) scan()
      }}
    >
      <Dialog.Trigger asChild>
        <button
          type="button"
          className="inline-flex items-center gap-1.5 rounded-lg border border-slate-200 px-2.5 py-2 text-xs text-slate-600 transition hover:border-emerald-300/20 hover:bg-emerald-300/[0.05] hover:text-emerald-700"
        >
          <Terminal size={14} /> SSH 检测
        </button>
      </Dialog.Trigger>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-40 bg-black/65 backdrop-blur-sm" />
        <Dialog.Content className="dialog-content fixed top-1/2 left-1/2 z-50 max-h-[90vh] w-[calc(100%-2rem)] max-w-lg -translate-x-1/2 -translate-y-1/2 overflow-y-auto rounded-2xl border border-slate-200 bg-white p-6 shadow-2xl shadow-black/50 focus:outline-none">
          <div className="flex items-start justify-between gap-4">
            <div>
              <Dialog.Title className="text-lg font-semibold text-slate-900">连接 {host.name}</Dialog.Title>
              <Dialog.Description className="mt-1.5 font-mono text-xs text-slate-500">
                {host.username}@{host.address}:{host.ssh_port}
              </Dialog.Description>
            </div>
            <Dialog.Close asChild>
              <button type="button" aria-label="关闭" className="rounded-lg p-1.5 text-slate-500 hover:bg-slate-100 hover:text-slate-800">
                <X size={18} />
              </button>
            </Dialog.Close>
          </div>

          <div className="mt-6">
            {testMutation.isPending && <Progress label="正在扫描 SSH Host Key…" />}

            {testMutation.isError && (
              <ResultPanel tone="error" icon={AlertTriangle} title="SSH 连接失败">
                <p>{errorMessage(testMutation.error)}</p>
                <p className="mt-2 text-slate-500">请确认公钥已添加，并检查地址、端口和网络。</p>
              </ResultPanel>
            )}

            {report && !report.connected && (
              <ResultPanel
                tone={report.host_key.state === 'changed' ? 'error' : 'warning'}
                icon={Fingerprint}
                title={report.host_key.state === 'changed' ? 'Host Key 已变化' : '确认 SSH Host Key'}
              >
                <p>
                  请在目标主机控制台核对以下 {report.host_key.algorithm} 指纹，确认一致后再信任。
                </p>
                <code className="mt-3 block overflow-x-auto rounded-lg bg-slate-100 px-3 py-2.5 font-mono text-xs text-slate-800">
                  {report.host_key.fingerprint}
                </code>
                {report.host_key.state === 'changed' && (
                  <p className="mt-3 font-medium text-rose-700">不要在无法解释 Host Key 变化时继续。</p>
                )}
                <button
                  type="button"
                  disabled={trustMutation.isPending}
                  onClick={() => trustMutation.mutate(report.host_key.fingerprint)}
                  className="mt-4 inline-flex items-center gap-2 rounded-xl bg-amber-300 px-3.5 py-2.5 text-xs font-semibold text-amber-950 hover:bg-amber-200 disabled:opacity-60"
                >
                  {trustMutation.isPending ? <LoaderCircle size={14} className="animate-spin" /> : <ShieldCheck size={14} />}
                  指纹一致，信任并连接
                </button>
              </ResultPanel>
            )}

            {report?.connected && environmentMutation.isPending && <Progress label="SSH 已连接，正在检测系统环境…" />}

            {environmentMutation.isError && (
              <ResultPanel tone="error" icon={AlertTriangle} title="环境检测失败">
                <p>{errorMessage(environmentMutation.error)}</p>
              </ResultPanel>
            )}

            {environment && <EnvironmentResult report={environment} />}
          </div>

          <div className="mt-6 flex justify-end border-t border-slate-200 pt-5">
            <button
              type="button"
              disabled={pending}
              onClick={scan}
              className="inline-flex items-center gap-1.5 rounded-lg px-2.5 py-2 text-xs text-slate-600 hover:bg-slate-100 hover:text-slate-800 disabled:opacity-40"
            >
              <RefreshCw size={13} /> 重新检测
            </button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}

function Progress({ label }: { label: string }) {
  return (
    <div className="flex min-h-36 flex-col items-center justify-center rounded-xl border border-slate-200 bg-slate-50 text-center">
      <LoaderCircle size={22} className="animate-spin text-emerald-600" />
      <p className="mt-3 text-sm text-slate-600">{label}</p>
    </div>
  )
}

function ResultPanel({
  tone,
  icon: Icon,
  title,
  children,
}: {
  tone: 'warning' | 'error'
  icon: typeof AlertTriangle
  title: string
  children: React.ReactNode
}) {
  const styles = tone === 'error'
    ? 'border-rose-400/15 bg-rose-400/[0.06] text-rose-800'
    : 'border-amber-300/15 bg-amber-300/[0.06] text-amber-800'
  return (
    <div className={`rounded-xl border p-4 ${styles}`}>
      <div className="flex items-center gap-2 text-sm font-medium"><Icon size={17} /> {title}</div>
      <div className="mt-3 text-xs leading-5 text-slate-600">{children}</div>
    </div>
  )
}

function EnvironmentResult({ report }: { report: EnvironmentReport }) {
  return (
    <div className={`rounded-xl border p-4 ${report.supported ? 'border-emerald-300/15 bg-emerald-300/[0.05]' : 'border-rose-400/15 bg-rose-400/[0.05]'}`}>
      <div className="flex items-center gap-2 text-sm font-medium text-slate-900">
        {report.supported ? <CheckCircle2 size={18} className="text-emerald-600" /> : <AlertTriangle size={18} className="text-rose-600" />}
        {report.supported ? '环境受支持' : '不支持的系统环境'}
      </div>
      <dl className="mt-4 grid grid-cols-[7rem_1fr] gap-x-3 gap-y-2 text-xs">
        <dt className="text-slate-500">平台</dt><dd className="text-slate-700">{platformName(report)}</dd>
        <dt className="text-slate-500">主机名</dt><dd className="text-slate-700">{report.platform.hostname}</dd>
        <dt className="text-slate-500">systemd</dt><dd className="truncate text-slate-700">{report.systemd_version ?? '未检测到'}</dd>
        <dt className="text-slate-500">NUT</dt><dd className="text-slate-700">{report.nut_server_installed && report.nut_client_installed ? report.platform.nut_version ?? '已安装' : '未完整安装'}</dd>
      </dl>
    </div>
  )
}

function platformName(report: EnvironmentReport) {
  const labels = {
    debian: 'Debian',
    proxmox_ve: 'Proxmox VE',
    proxmox_backup_server: 'Proxmox Backup Server',
    unsupported: 'Unsupported',
  }
  const version = report.platform.product_version ?? report.platform.os_version
  return `${labels[report.platform.kind]} ${version}`
}
