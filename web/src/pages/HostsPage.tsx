import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  AlertTriangle,
  CircleHelp,
  LoaderCircle,
  MoreHorizontal,
  Server,
  Trash2,
} from 'lucide-react'
import * as AlertDialog from 'radix-ui/alert-dialog'
import * as DropdownMenu from 'radix-ui/dropdown-menu'
import { toast } from 'sonner'
import { AddHostDialog } from '../components/AddHostDialog.tsx'
import { SshConnectionDialog } from '../components/SshConnectionDialog.tsx'
import { SshPublicKeyCard } from '../components/SshPublicKeyCard.tsx'
import { NutInstallDialog } from '../components/NutInstallDialog.tsx'
import { UpsScanDialog } from '../components/UpsScanDialog.tsx'
import { ServerConfigDialog } from '../components/ServerConfigDialog.tsx'
import { ShutdownConfigDialog } from '../components/ShutdownConfigDialog.tsx'
import { ClientConfigDialog } from '../components/ClientConfigDialog.tsx'
import {
  bindingsQueryKey,
  deleteHost,
  errorMessage,
  hostsQueryKey,
  listBindings,
  listHosts,
  listServers,
  serversQueryKey,
} from '../lib/api.ts'
import type { Host, NutBindingRecord, NutServerRecord, PlatformKind } from '../lib/types.ts'

export function HostsPage() {
  const hosts = useQuery({ queryKey: hostsQueryKey, queryFn: listHosts })
  const servers = useQuery({ queryKey: serversQueryKey, queryFn: listServers })
  const bindings = useQuery({ queryKey: bindingsQueryKey, queryFn: listBindings })

  return (
    <div>
      <header className="flex flex-col justify-between gap-5 sm:flex-row sm:items-end">
        <div>
          <p className="mb-2 text-xs font-medium tracking-[0.16em] text-emerald-600/80 uppercase">Inventory</p>
          <h1 className="text-2xl font-semibold tracking-tight text-slate-900 lg:text-3xl">主机管理</h1>
          <p className="mt-2 text-sm text-slate-500">维护 Server 与 Client 的连接信息和配置状态。</p>
        </div>
        <AddHostDialog />
      </header>

      <SshPublicKeyCard />

      <section className="mt-5 overflow-hidden rounded-2xl border border-slate-200 bg-white">
        <div className="flex items-center justify-between border-b border-slate-200 px-5 py-4">
          <div className="flex items-center gap-2 text-sm font-medium text-slate-700">
            <Server size={16} className="text-emerald-600" />
            已添加主机
          </div>
          <span className="text-xs text-slate-500">{hosts.data?.length ?? 0} 台</span>
        </div>

        {hosts.isPending ? (
          <div className="grid min-h-56 place-items-center text-sm text-slate-500">
            <LoaderCircle size={20} className="animate-spin" />
          </div>
        ) : hosts.isError ? (
          <div className="grid min-h-56 place-items-center px-6 text-center">
            <div>
              <AlertTriangle className="mx-auto text-rose-600" size={24} />
              <p className="mt-3 text-sm text-slate-700">无法读取主机列表</p>
              <p className="mt-1 text-xs text-slate-500">{errorMessage(hosts.error)}</p>
            </div>
          </div>
        ) : hosts.data.length === 0 ? (
          <div className="grid min-h-72 place-items-center px-6 text-center">
            <div className="max-w-sm">
              <div className="mx-auto grid size-12 place-items-center rounded-2xl bg-slate-300/[0.06] text-slate-600">
                <Server size={22} strokeWidth={1.6} />
              </div>
              <h2 className="mt-4 text-sm font-medium text-slate-800">还没有管理主机</h2>
              <p className="mt-2 text-xs leading-5 text-slate-500">
                首先添加连接 USB UPS 的主机作为 Server，或添加一台待配置的 Client。
              </p>
            </div>
          </div>
        ) : (
          <div className="divide-y divide-slate-100">
            {hosts.data.map((host) => (
              <HostRow
                key={host.id}
                host={host}
                server={servers.data?.find((server) => server.host_id === host.id) ?? null}
                servers={servers.data ?? []}
                hosts={hosts.data}
                binding={bindings.data?.find((binding) => binding.client_host_id === host.id) ?? null}
              />
            ))}
          </div>
        )}
      </section>
    </div>
  )
}

function HostRow({
  host,
  server,
  servers,
  hosts,
  binding,
}: {
  host: Host
  server: NutServerRecord | null
  servers: NutServerRecord[]
  hosts: Host[]
  binding: NutBindingRecord | null
}) {
  const queryClient = useQueryClient()
  const deleteMutation = useMutation({
    mutationFn: () => deleteHost(host.id),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: hostsQueryKey })
      toast.success(`已从 NWM 删除 ${host.name}`, {
        description: '远端主机和 NUT 配置未被修改。',
      })
    },
    onError: (error) => toast.error(errorMessage(error)),
  })

  return (
    <div className="grid gap-4 px-5 py-4 transition hover:bg-slate-50 lg:grid-cols-[minmax(0,1.4fr)_0.65fr_0.75fr_auto] lg:items-center">
      <div className="flex min-w-0 items-center gap-3">
        <div className={`grid size-10 shrink-0 place-items-center rounded-xl ${host.role === 'server' ? 'bg-cyan-300/10 text-cyan-700' : 'bg-violet-300/10 text-violet-600'}`}>
          <Server size={18} />
        </div>
        <div className="min-w-0">
          <p className="truncate text-sm font-medium text-slate-800">{host.name}</p>
          <p className="mt-1 truncate font-mono text-[11px] text-slate-500">
            {host.username}@{host.address}:{host.ssh_port}
          </p>
        </div>
      </div>

      <div>
        <p className="text-[10px] tracking-wider text-slate-600 uppercase lg:hidden">角色</p>
        <span className={`mt-1 inline-flex rounded-full border px-2.5 py-1 text-[10px] font-semibold uppercase tracking-wider ${host.role === 'server' ? 'border-cyan-300/15 bg-cyan-300/[0.06] text-cyan-700' : 'border-violet-300/15 bg-violet-300/[0.06] text-violet-600'}`}>
          {host.role}
        </span>
      </div>

      <div className="flex items-center gap-2 text-xs text-slate-500">
        <CircleHelp size={15} />
        {host.platform ? platformLabel(host.platform.kind) : '待检测'}
      </div>

      <div className="flex flex-wrap items-center gap-2 lg:justify-end lg:flex-nowrap">
        <SshConnectionDialog host={host} />
        <NutInstallDialog host={host} />
        {host.role === 'server' && <UpsScanDialog host={host} />}
        {host.role === 'server' && server && <ShutdownConfigDialog server={server} />}
        {host.role === 'server' && server && <ServerConfigDialog server={server} />}
        {host.role === 'client' && <ClientConfigDialog host={host} servers={servers} hosts={hosts} binding={binding} />}

        <AlertDialog.Root>
        <DropdownMenu.Root>
          <DropdownMenu.Trigger asChild>
            <button
              type="button"
              aria-label={`${host.name} 操作`}
              className="rounded-lg p-2 text-slate-500 transition hover:bg-slate-100 hover:text-slate-800"
            >
              <MoreHorizontal size={18} />
            </button>
          </DropdownMenu.Trigger>
          <DropdownMenu.Portal>
            <DropdownMenu.Content
              align="end"
              sideOffset={6}
              className="z-30 min-w-40 rounded-xl border border-slate-200 bg-white p-1.5 text-sm shadow-2xl shadow-black/40"
            >
              <AlertDialog.Trigger asChild>
                <DropdownMenu.Item
                  onSelect={(event) => event.preventDefault()}
                  className="flex select-none items-center gap-2 rounded-lg px-2.5 py-2 text-xs text-rose-600 outline-none data-[highlighted]:bg-rose-400/10"
                >
                  <Trash2 size={14} /> 删除本地记录
                </DropdownMenu.Item>
              </AlertDialog.Trigger>
            </DropdownMenu.Content>
          </DropdownMenu.Portal>
        </DropdownMenu.Root>

        <AlertDialog.Portal>
          <AlertDialog.Overlay className="fixed inset-0 z-40 bg-black/65 backdrop-blur-sm" />
          <AlertDialog.Content className="dialog-content fixed top-1/2 left-1/2 z-50 w-[calc(100%-2rem)] max-w-md -translate-x-1/2 -translate-y-1/2 rounded-2xl border border-slate-200 bg-white p-6 shadow-2xl shadow-black/50 focus:outline-none">
            <AlertDialog.Title className="text-lg font-semibold text-slate-900">
              删除 {host.name}？
            </AlertDialog.Title>
            <AlertDialog.Description className="mt-2 text-sm leading-6 text-slate-600">
              只删除 NWM 中的本地记录。远端 authorized_keys、NUT 软件与最后生效配置都会保留。
            </AlertDialog.Description>
            <div className="mt-6 flex justify-end gap-3">
              <AlertDialog.Cancel asChild>
                <button type="button" className="rounded-xl border border-slate-200 px-4 py-2.5 text-sm text-slate-700 hover:bg-slate-100">
                  取消
                </button>
              </AlertDialog.Cancel>
              <AlertDialog.Action asChild>
                <button
                  type="button"
                  disabled={deleteMutation.isPending}
                  onClick={() => deleteMutation.mutate()}
                  className="inline-flex items-center gap-2 rounded-xl bg-rose-400 px-4 py-2.5 text-sm font-semibold text-rose-950 hover:bg-rose-300 disabled:opacity-60"
                >
                  {deleteMutation.isPending && <LoaderCircle size={15} className="animate-spin" />}
                  删除记录
                </button>
              </AlertDialog.Action>
            </div>
          </AlertDialog.Content>
        </AlertDialog.Portal>
        </AlertDialog.Root>
      </div>
    </div>
  )
}

function platformLabel(kind: PlatformKind) {
  return {
    debian: 'Debian 13',
    proxmox_ve: 'Proxmox VE 9',
    proxmox_backup_server: 'PBS 4',
    unsupported: '不支持',
  }[kind]
}
