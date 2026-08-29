export type HostRole = 'server' | 'client'

export type PlatformKind =
  | 'debian'
  | 'proxmox_ve'
  | 'proxmox_backup_server'
  | 'unsupported'

export interface PlatformInfo {
  kind: PlatformKind
  os_version: string
  product_version: string | null
  hostname: string
  nut_version: string | null
}

export interface Host {
  id: string
  name: string
  address: string
  ssh_port: number
  username: string
  role: HostRole
  platform: PlatformInfo | null
  created_at: string
  updated_at: string
}

export interface Session {
  authenticated: boolean
  username: string
  default_credentials: boolean
}

export interface CreateHostInput {
  name: string
  address: string
  ssh_port: number
  username: string
  role: HostRole
}

export interface DeleteHostResult {
  remote_modified: false
  public_key_removed: false
  nut_uninstalled: false
}

export interface SshPublicKey {
  algorithm: 'ssh-ed25519'
  public_key: string
}

export type HostKeyState = 'trusted' | 'confirmation_required' | 'changed'

export interface HostKeyInspection {
  state: HostKeyState
  algorithm: string
  fingerprint: string
}

export interface SshTestReport {
  connected: boolean
  host_key: HostKeyInspection
}

export interface EnvironmentReport {
  platform: PlatformInfo
  supported: boolean
  systemd_version: string | null
  nut_server_installed: boolean
  nut_client_installed: boolean
  nut_server_version: string | null
  nut_client_version: string | null
}

export interface NutInstallStatus {
  role: HostRole
  platform: PlatformInfo
  package: 'nut'
  installed: boolean
  version: string | null
  install_command: string
  automatic_install_available: boolean
  already_installed: boolean
}

export type OperationState = 'pending' | 'running' | 'succeeded' | 'failed'

export interface Operation {
  id: string
  host_id: string | null
  kind: string
  state: OperationState
  progress: number
  error_code: string | null
  error_detail: string | null
  result: unknown | null
  created_at: string
  updated_at: string
}

export interface OperationAccepted {
  operation_id: string
}

export type UsbScanFormat = 'parsable' | 'nut_conf'

export interface UsbScanCandidate {
  index: number
  driver: string
  port: string
  vendor: string | null
  product: string | null
  serial: string | null
  vendor_id: string | null
  product_id: string | null
  bus: string | null
  device: string | null
  selectors: Record<string, string>
}

export interface UsbScanResult {
  format: UsbScanFormat
  scanned_at: string
  candidates: UsbScanCandidate[]
}

export type ApplyState = 'unconfigured' | 'pending' | 'applying' | 'applied' | 'removing' | 'failed'

export interface UpsDeviceRecord {
  id: string
  name: string
  driver: string
  port: string
  selectors: Record<string, string>
}

export interface NutServerRecord {
  id: string
  host_id: string
  ups_name: string
  listen_address: string
  listen_port: number
  enabled: boolean
  apply_state: ApplyState
  applied_revision_id: string | null
  shutdown: ShutdownOptions
  device: UpsDeviceRecord
}

export interface ShutdownOptions {
  trigger_mode: 'battery_level' | 'on_battery_timer'
  battery_level_percent: number
  on_battery_seconds: number
  host_sync_seconds: number
  final_delay_seconds: number
  powerdown_enabled: boolean
}

export type ManagementConnectivity =
  | 'connected'
  | 'disconnected'
  | 'host_key_mismatch'
  | 'authentication_failed'
  | 'unknown'

export type ProtectionHealth = 'active' | 'degraded' | 'unknown' | 'unconfigured'
export type PowerSource = 'mains' | 'battery' | 'bypass' | 'off' | 'other' | 'unknown'

export interface UpsObservation {
  ups_id: string
  reachable: boolean
  power_source: PowerSource
  battery_condition: 'normal' | 'low' | 'depleted' | 'replace' | 'unknown'
  status_flags: string[]
  charge_percent: number | null
  runtime_seconds: number | null
  load_percent: number | null
  manufacturer: string | null
  model: string | null
  serial: string | null
  raw: Record<string, string>
  observed_at: string
  error: unknown | null
}

export interface DashboardSnapshot {
  server: NutServerRecord | null
  ups: UpsObservation | null
  management: ManagementConnectivity
  protection: ProtectionHealth
  services: {
    driver_active: boolean
    server_active: boolean
    monitor_active: boolean
  } | null
  observed_at: string
  last_verified_at: string | null
  error: unknown | null
}

export interface NutBindingRecord {
  id: string
  server_id: string
  client_host_id: string
  username: string
  apply_state: ApplyState
  applied_revision_id: string | null
  created_at: string
  updated_at: string
}

export type ConfigOwnership = 'distribution_default' | 'managed_unchanged' | 'unmanaged_existing' | 'managed_modified'

export interface ConfigConflict {
  path: string
  reason: string
}

export interface ConfigPreviewFile {
  path: string
  current: string
  candidate: string
}

export interface ConfigPreview {
  role: HostRole
  ownership: ConfigOwnership
  files: ConfigPreviewFile[]
  services: string[]
  conflicts: ConfigConflict[]
  takeover_required: boolean
  snapshot_hash: string
  takeover_allowed: boolean
  takeover_block_reason: string | null
  takeover_warning: string | null
  role_transition_required: boolean
}

export interface BindingConfigPreview {
  server: ConfigPreview
  client: ConfigPreview
}
