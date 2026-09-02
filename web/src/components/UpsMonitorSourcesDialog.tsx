import { useMutation, useQueryClient } from '@tanstack/react-query'
import { CheckCircle2, LoaderCircle, Pencil, Plus, Radar, RefreshCw, Settings2, Trash2, X } from 'lucide-react'
import * as Dialog from 'radix-ui/dialog'
import { useState } from 'react'
import type { ReactNode } from 'react'
import { toast } from 'sonner'
import {
  createUpsMonitorSource,
  deleteUpsMonitorSource,
  discoverUpsMonitorSource,
  errorMessage,
  testUpsMonitorSource,
  updateUpsMonitorSource,
  upsMonitorQueryKey,
} from '../lib/api.ts'
import type { UpsMonitorSource, UpsMonitorSourceInput } from '../lib/types.ts'

const emptyForm: UpsMonitorSourceInput = { name: '', address: '', port: 3493, enabled: true }

export function UpsMonitorSourcesDialog({ sources }: { sources: UpsMonitorSource[] }) {
  const [open, setOpen] = useState(false)
  const [editingId, setEditingId] = useState<string | null>(null)
  const [form, setForm] = useState<UpsMonitorSourceInput>(emptyForm)
  const queryClient = useQueryClient()
  const refresh = () => queryClient.invalidateQueries({ queryKey: upsMonitorQueryKey })
  const save = useMutation({
    mutationFn: (resetDevices: boolean) => editingId
      ? updateUpsMonitorSource(editingId, { ...form, reset_devices: resetDevices })
      : createUpsMonitorSource(form),
    onSuccess: (source, resetDevices) => {
      void refresh()
      setEditingId(null)
      setForm(emptyForm)
      toast.success(resetDevices
        ? `数据源“${source.name}”已重置，后台将重新发现 UPS`
        : `数据源“${source.name}”已保存，后台将自动发现 UPS`)
    },
    onError: (error) => toast.error(errorMessage(error)),
  })
  const test = useMutation({
    mutationFn: testUpsMonitorSource,
    onSuccess: (result) => result.reachable
      ? toast.success(`连接成功，发现 ${result.devices.length} 台 UPS`)
      : toast.error(result.error ?? '连接失败'),
    onError: (error) => toast.error(errorMessage(error)),
  })
  const discover = useMutation({
    mutationFn: discoverUpsMonitorSource,
    onSuccess: (devices) => { void refresh(); toast.success(`发现并同步了 ${devices.length} 台 UPS`) },
    onError: (error) => toast.error(errorMessage(error)),
  })
  const remove = useMutation({
    mutationFn: deleteUpsMonitorSource,
    onSuccess: () => { void refresh(); toast.success('数据源及其监控历史已删除') },
    onError: (error) => toast.error(errorMessage(error)),
  })

  const edit = (source: UpsMonitorSource) => {
    setEditingId(source.id)
    setForm({ name: source.name, address: source.address, port: source.port, enabled: source.enabled })
  }
  const cancel = () => { setEditingId(null); setForm(emptyForm) }
  const submit = () => {
    const original = sources.find((source) => source.id === editingId)
    const endpointChanged = Boolean(original)
      && (original?.address !== form.address.trim() || original.port !== form.port)
    if (endpointChanged && !window.confirm('修改地址或端口会重置该数据源已发现的设备，并永久删除相关快照、历史和事件。确定继续吗？')) {
      return
    }
    save.mutate(endpointChanged)
  }

  return (
    <Dialog.Root open={open} onOpenChange={setOpen}>
      <Dialog.Trigger asChild>
        <button type="button" className="inline-flex items-center gap-2 rounded-xl border border-slate-200 bg-white px-3.5 py-2.5 text-sm font-medium text-slate-700 shadow-sm hover:bg-slate-50">
          <Settings2 size={16} /> 数据源
        </button>
      </Dialog.Trigger>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-40 bg-black/45 backdrop-blur-sm" />
        <Dialog.Content className="dialog-content fixed top-1/2 left-1/2 z-50 max-h-[92vh] w-[calc(100%-2rem)] max-w-4xl -translate-x-1/2 -translate-y-1/2 overflow-y-auto rounded-2xl border border-slate-200 bg-white p-6 shadow-2xl focus:outline-none">
          <div className="flex items-start justify-between gap-4">
            <div><Dialog.Title className="text-lg font-semibold">UPS 数据源</Dialog.Title></div>
            <Dialog.Close asChild><button type="button" aria-label="关闭" className="rounded-lg p-1.5 text-slate-500 hover:bg-slate-100"><X size={18} /></button></Dialog.Close>
          </div>

          <div className="mt-6 grid gap-6 lg:grid-cols-[1fr_0.9fr]">
            <section>
              <h3 className="text-xs font-semibold tracking-wide text-slate-500 uppercase">已配置数据源</h3>
              <div className="mt-3 space-y-3">
                {sources.length === 0 && <p className="rounded-xl border border-dashed border-slate-200 p-6 text-center text-sm text-slate-500">尚未添加数据源</p>}
                {sources.map((source) => (
                  <article key={source.id} className="rounded-xl border border-slate-200 p-4">
                    <div className="flex items-start justify-between gap-3">
                      <div className="min-w-0"><p className="truncate text-sm font-semibold text-slate-800">{source.name}</p><p className="mt-1 text-xs text-slate-500">{source.address}:{source.port}</p></div>
                      <span className={`rounded-full px-2 py-1 text-[10px] ${source.enabled ? 'bg-emerald-50 text-emerald-700' : 'bg-slate-100 text-slate-500'}`}>{source.enabled ? '已启用' : '已停用'}</span>
                    </div>
                    {source.last_error && <p className="mt-3 break-all rounded-lg bg-rose-50 px-3 py-2 text-[11px] text-rose-700">{source.last_error}</p>}
                    <div className="mt-3 flex flex-wrap gap-2">
                      <ActionButton icon={CheckCircle2} label="测试" loading={test.isPending && test.variables === source.id} onClick={() => test.mutate(source.id)} />
                      <ActionButton icon={RefreshCw} label="发现" loading={discover.isPending && discover.variables === source.id} onClick={() => discover.mutate(source.id)} />
                      <ActionButton icon={Pencil} label="编辑" onClick={() => edit(source)} />
                      <ActionButton icon={Trash2} label="删除" danger onClick={() => {
                        if (window.confirm(`删除“${source.name}”及其全部历史数据？此操作无法撤销。`)) remove.mutate(source.id)
                      }} />
                    </div>
                  </article>
                ))}
              </div>
            </section>

            <section className="rounded-xl border border-slate-200 bg-slate-50 p-5">
              <h3 className="flex items-center gap-2 text-sm font-semibold text-slate-800">{editingId ? <Pencil size={16} /> : <Plus size={16} />}{editingId ? '编辑数据源' : '添加数据源'}</h3>
              <div className="mt-5 space-y-4">
                <Field label="显示名称"><input value={form.name} maxLength={100} onChange={(event) => setForm({ ...form, name: event.target.value })} placeholder="例如：机房 NUT Server" className="field-input" /></Field>
                <Field label="NAS 地址"><input value={form.address} onChange={(event) => setForm({ ...form, address: event.target.value })} placeholder="192.168.1.1" className="field-input" /></Field>
                <Field label="NUT TCP 端口"><input type="number" min={1} max={65535} value={form.port} onChange={(event) => setForm({ ...form, port: Number(event.target.value) })} className="field-input" /></Field>
                <label className="flex items-center gap-3 text-sm text-slate-700"><input type="checkbox" checked={form.enabled} onChange={(event) => setForm({ ...form, enabled: event.target.checked })} className="accent-emerald-600" />启用后台采集</label>
                <button type="button" disabled={save.isPending || !form.name.trim() || !form.address.trim() || form.port < 1 || form.port > 65535} onClick={submit} className="inline-flex w-full items-center justify-center gap-2 rounded-xl bg-emerald-600 px-4 py-3 text-sm font-semibold text-white hover:bg-emerald-500 disabled:opacity-50">
                  {save.isPending ? <LoaderCircle size={17} className="animate-spin" /> : <Radar size={17} />} 保存数据源
                </button>
                {editingId && <button type="button" onClick={cancel} className="w-full text-xs text-slate-500 hover:text-slate-800">取消编辑</button>}
              </div>
            </section>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}

function Field({ label, children }: { label: string; children: ReactNode }) {
  return <label className="block text-xs font-medium text-slate-600">{label}<span className="mt-2 block">{children}</span></label>
}

function ActionButton({ icon: Icon, label, onClick, loading = false, danger = false }: { icon: typeof Pencil; label: string; onClick: () => void; loading?: boolean; danger?: boolean }) {
  return <button type="button" disabled={loading} onClick={onClick} className={`inline-flex items-center gap-1.5 rounded-lg border px-2.5 py-1.5 text-xs ${danger ? 'border-rose-200 text-rose-700 hover:bg-rose-50' : 'border-slate-200 text-slate-600 hover:bg-slate-50'}`}>{loading ? <LoaderCircle size={13} className="animate-spin" /> : <Icon size={13} />}{label}</button>
}
