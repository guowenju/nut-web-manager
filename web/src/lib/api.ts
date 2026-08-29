import type {
  CreateHostInput,
  DeleteHostResult,
  EnvironmentReport,
  Host,
  Session,
  SshPublicKey,
  SshTestReport,
  HostKeyInspection,
  NutInstallStatus,
  Operation,
  OperationAccepted,
  NutServerRecord,
  NutBindingRecord,
  UsbScanCandidate,
  ConfigPreview,
  BindingConfigPreview,
  DashboardSnapshot,
  ShutdownOptions,
} from './types.ts'

interface ErrorEnvelope {
  error?: {
    code?: string
    message?: string
  }
}

export class ApiError extends Error {
  readonly status: number
  readonly code: string

  constructor(status: number, code: string, message: string) {
    super(message)
    this.name = 'ApiError'
    this.status = status
    this.code = code
  }
}

async function apiRequest<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`/api/v1${path}`, {
    ...init,
    credentials: 'same-origin',
    headers: {
      ...(init?.body ? { 'Content-Type': 'application/json' } : {}),
      ...init?.headers,
    },
  })

  const body = (await response.json().catch(() => null)) as T | ErrorEnvelope | null
  if (!response.ok) {
    const envelope = body as ErrorEnvelope | null
    throw new ApiError(
      response.status,
      envelope?.error?.code ?? 'RequestFailed',
      envelope?.error?.message ?? `请求失败（HTTP ${response.status}）`,
    )
  }

  return body as T
}

export const sessionQueryKey = ['session'] as const
export const hostsQueryKey = ['hosts'] as const
export const sshPublicKeyQueryKey = ['ssh-public-key'] as const
export const serversQueryKey = ['servers'] as const
export const bindingsQueryKey = ['bindings'] as const
export const dashboardQueryKey = ['dashboard'] as const

export async function getSession(): Promise<Session | null> {
  try {
    return await apiRequest<Session>('/auth/session')
  } catch (error) {
    if (error instanceof ApiError && error.status === 401) {
      return null
    }
    throw error
  }
}

export function login(input: { username: string; password: string }) {
  return apiRequest<Session>('/auth/login', {
    method: 'POST',
    body: JSON.stringify(input),
  })
}

export function logout() {
  return apiRequest<Session>('/auth/logout', { method: 'POST' })
}

export function listHosts() {
  return apiRequest<Host[]>('/hosts')
}

export function createHost(input: CreateHostInput) {
  return apiRequest<Host>('/hosts', {
    method: 'POST',
    body: JSON.stringify(input),
  })
}

export function deleteHost(id: string) {
  return apiRequest<DeleteHostResult>(`/hosts/${id}`, { method: 'DELETE' })
}

export function getSshPublicKey() {
  return apiRequest<SshPublicKey>('/ssh/public-key')
}

export function testSsh(id: string) {
  return apiRequest<SshTestReport>(`/hosts/${id}/ssh/test`, { method: 'POST' })
}

export function trustSshHostKey(id: string, fingerprint: string) {
  return apiRequest<HostKeyInspection>(`/hosts/${id}/ssh/trust`, {
    method: 'POST',
    body: JSON.stringify({ fingerprint }),
  })
}

export function detectHostEnvironment(id: string) {
  return apiRequest<EnvironmentReport>(`/hosts/${id}/environment`)
}

export function getNutInstallStatus(id: string) {
  return apiRequest<NutInstallStatus>(`/hosts/${id}/nut/install`)
}

export function installNut(id: string) {
  return apiRequest<OperationAccepted>(`/hosts/${id}/nut/install`, {
    method: 'POST',
    body: JSON.stringify({ confirmed: true }),
  })
}

export function deactivateNut(id: string) {
  return apiRequest<OperationAccepted>(`/hosts/${id}/nut/deactivate`, {
    method: 'POST',
    body: JSON.stringify({ confirmed: true }),
  })
}

export function getOperation(id: string) {
  return apiRequest<Operation>(`/operations/${id}`)
}

export function scanUsbUps(id: string) {
  return apiRequest<OperationAccepted>(`/hosts/${id}/nut/scan`, { method: 'POST' })
}

export function listServers() {
  return apiRequest<NutServerRecord[]>('/servers')
}

export function getDashboard() {
  return apiRequest<DashboardSnapshot>('/dashboard')
}

export function updateServerShutdown(serverId: string, input: ShutdownOptions) {
  return apiRequest<NutServerRecord>(`/servers/${serverId}/shutdown`, {
    method: 'POST',
    body: JSON.stringify(input),
  })
}

export function selectServerDevice(hostId: string, candidate: UsbScanCandidate, upsName = 'ups') {
  return apiRequest<NutServerRecord>('/servers', {
    method: 'POST',
    body: JSON.stringify({ host_id: hostId, ups_name: upsName, candidate }),
  })
}

export function previewServerConfig(serverId: string) {
  return apiRequest<ConfigPreview>(`/servers/${serverId}/config/preview`)
}

export interface TakeoverSnapshotInput {
  host_id: string
  snapshot_hash: string
}

export function applyServerConfig(serverId: string, takeoverSnapshots: TakeoverSnapshotInput[] = []) {
  return apiRequest<OperationAccepted>(`/servers/${serverId}/config/apply`, {
    method: 'POST',
    body: JSON.stringify({ confirmed: true, takeover: takeoverSnapshots.length > 0, takeover_snapshots: takeoverSnapshots }),
  })
}

export function listBindings() {
  return apiRequest<NutBindingRecord[]>('/bindings')
}

export function createBinding(serverId: string, clientHostId: string) {
  return apiRequest<NutBindingRecord>('/bindings', {
    method: 'POST',
    body: JSON.stringify({ server_id: serverId, client_host_id: clientHostId }),
  })
}

export function previewBindingConfig(bindingId: string) {
  return apiRequest<BindingConfigPreview>(`/bindings/${bindingId}/config/preview`)
}

export function applyBindingConfig(bindingId: string, takeoverSnapshots: TakeoverSnapshotInput[] = []) {
  return apiRequest<OperationAccepted>(`/bindings/${bindingId}/config/apply`, {
    method: 'POST',
    body: JSON.stringify({ confirmed: true, takeover: takeoverSnapshots.length > 0, takeover_snapshots: takeoverSnapshots }),
  })
}

export function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : '发生未知错误'
}
