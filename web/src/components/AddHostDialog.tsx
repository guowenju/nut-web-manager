import { zodResolver } from '@hookform/resolvers/zod'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { ChevronDown, LoaderCircle, Plus, X } from 'lucide-react'
import * as Dialog from 'radix-ui/dialog'
import { useState } from 'react'
import { useForm } from 'react-hook-form'
import { toast } from 'sonner'
import { z } from 'zod'
import { createHost, errorMessage, hostsQueryKey } from '../lib/api.ts'

const hostSchema = z.object({
  name: z.string().trim().min(1, '请输入主机名称').max(80, '名称不能超过 80 个字符'),
  address: z.string().trim().min(1, '请输入 IP 地址或主机名').max(255),
  ssh_port: z.number().int().min(1).max(65535),
  username: z.string().trim().min(1, '请输入 SSH 用户名').max(64),
  role: z.enum(['server', 'client']),
})

type HostForm = z.infer<typeof hostSchema>

const defaults: HostForm = {
  name: '',
  address: '',
  ssh_port: 22,
  username: 'root',
  role: 'server',
}

export function AddHostDialog() {
  const [open, setOpen] = useState(false)
  const queryClient = useQueryClient()
  const {
    register,
    handleSubmit,
    reset,
    formState: { errors },
  } = useForm<HostForm>({
    resolver: zodResolver(hostSchema),
    defaultValues: defaults,
  })
  const createMutation = useMutation({
    mutationFn: createHost,
    onSuccess: (host) => {
      void queryClient.invalidateQueries({ queryKey: hostsQueryKey })
      toast.success(`已添加 ${host.name}`)
      reset(defaults)
      setOpen(false)
    },
    onError: (error) => toast.error(errorMessage(error)),
  })

  return (
    <Dialog.Root
      open={open}
      onOpenChange={(nextOpen) => {
        setOpen(nextOpen)
        if (!nextOpen && !createMutation.isPending) reset(defaults)
      }}
    >
      <Dialog.Trigger asChild>
        <button
          type="button"
          className="inline-flex items-center gap-2 rounded-xl bg-emerald-300 px-4 py-2.5 text-sm font-semibold text-emerald-950 shadow-lg shadow-emerald-950/20 transition hover:bg-emerald-200 focus:outline-none focus:ring-2 focus:ring-emerald-300/40"
        >
          <Plus size={17} strokeWidth={2.2} />
          添加主机
        </button>
      </Dialog.Trigger>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-40 bg-black/65 backdrop-blur-sm" />
        <Dialog.Content className="dialog-content fixed top-1/2 left-1/2 z-50 max-h-[90vh] w-[calc(100%-2rem)] max-w-lg -translate-x-1/2 -translate-y-1/2 overflow-y-auto rounded-2xl border border-slate-200 bg-white p-6 shadow-2xl shadow-black/50 focus:outline-none">
          <div className="flex items-start justify-between gap-4">
            <div>
              <Dialog.Title className="text-lg font-semibold text-slate-900">添加管理主机</Dialog.Title>
              <Dialog.Description className="mt-1.5 text-sm leading-6 text-slate-600">
                保存连接信息后，再进行 SSH Host Key 确认和环境检测。
              </Dialog.Description>
            </div>
            <Dialog.Close asChild>
              <button
                type="button"
                aria-label="关闭"
                className="rounded-lg p-1.5 text-slate-500 transition hover:bg-slate-100 hover:text-slate-800"
              >
                <X size={18} />
              </button>
            </Dialog.Close>
          </div>

          <form className="mt-6 space-y-4" onSubmit={handleSubmit((value) => createMutation.mutate(value))}>
            <Field label="主机名称" error={errors.name?.message}>
              <input {...register('name')} autoFocus placeholder="例如：PVE 主节点" className={inputClass} />
            </Field>
            <Field label="IP 地址或主机名" error={errors.address?.message}>
              <input {...register('address')} placeholder="192.168.1.10" className={inputClass} />
            </Field>
            <div className="grid grid-cols-2 gap-4">
              <Field label="SSH 端口" error={errors.ssh_port?.message}>
                <input
                  {...register('ssh_port', { valueAsNumber: true })}
                  type="number"
                  min={1}
                  max={65535}
                  className={inputClass}
                />
              </Field>
              <Field label="SSH 用户名" error={errors.username?.message}>
                <input {...register('username')} className={inputClass} />
              </Field>
            </div>
            <Field label="NUT 角色" error={errors.role?.message}>
              <div className="relative">
                <select {...register('role')} className={`${inputClass} appearance-none pr-10`}>
                  <option value="server">Server · 连接 USB UPS</option>
                  <option value="client">Client · 从 Server 获取状态</option>
                </select>
                <ChevronDown
                  size={16}
                  className="pointer-events-none absolute top-1/2 right-3 -translate-y-1/2 text-slate-500"
                />
              </div>
            </Field>

            <div className="mt-6 flex justify-end border-t border-slate-200 pt-5">
              <button
                type="submit"
                disabled={createMutation.isPending}
                className="inline-flex min-w-24 items-center justify-center gap-2 rounded-xl bg-emerald-300 px-4 py-2.5 text-sm font-semibold text-emerald-950 transition hover:bg-emerald-200 disabled:cursor-not-allowed disabled:opacity-60"
              >
                {createMutation.isPending && <LoaderCircle size={16} className="animate-spin" />}
                保存主机
              </button>
            </div>
          </form>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}

const inputClass =
  'w-full rounded-xl border border-slate-200 bg-slate-50 px-3.5 py-2.5 text-sm text-slate-900 outline-none transition placeholder:text-slate-600 focus:border-emerald-300/50 focus:ring-2 focus:ring-emerald-300/10'

function Field({
  label,
  error,
  children,
}: {
  label: string
  error?: string
  children: React.ReactNode
}) {
  return (
    <label className="block">
      <span className="mb-1.5 block text-xs font-medium text-slate-700">{label}</span>
      {children}
      {error && <span className="mt-1 block text-xs text-rose-600">{error}</span>}
    </label>
  )
}
