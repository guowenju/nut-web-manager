use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::Duration,
};

use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::Utc;
use nwm_common::{Host, HostRole};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
    time::timeout,
};

use crate::{
    persistence::{
        CredentialRecord, RevisionRecord, ServerRecord, ShutdownTriggerMode,
        is_durable_usb_selector,
    },
    ssh::{SshError, SshManager},
};

const CONFIG_TIMEOUT: Duration = Duration::from_secs(75);
const NETWORK_TIMEOUT: Duration = Duration::from_secs(5);
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigOwnership {
    DistributionDefault,
    ManagedUnchanged,
    UnmanagedExisting,
    ManagedModified,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ConfigPreview {
    pub role: HostRole,
    pub ownership: ConfigOwnership,
    pub files: Vec<ConfigPreviewFile>,
    pub services: Vec<String>,
    pub conflicts: Vec<ConfigConflict>,
    pub takeover_required: bool,
    pub snapshot_hash: String,
    pub takeover_allowed: bool,
    pub takeover_block_reason: Option<String>,
    pub takeover_warning: Option<String>,
    pub role_transition_required: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TakeoverPowerCheck {
    pub summary: String,
    pub warning: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ConfigConflict {
    pub path: String,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ConfigPreviewFile {
    pub path: String,
    pub current: String,
    pub candidate: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ConfigApplyOutcome {
    pub backup_path: String,
    pub manifest_json: String,
    pub local_validation: String,
    pub tcp_reachable: Option<bool>,
    pub warning: Option<String>,
    pub takeover: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct ConfigManifest {
    files: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct RemoteFile {
    path: String,
    exists: bool,
    mode: String,
    uid: String,
    gid: String,
    content: String,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error(transparent)]
    Ssh(#[from] SshError),
    #[error("configuration conflict in {path}: {reason}")]
    Conflict { path: String, reason: String },
    #[error("the remote NUT configuration changed after it was previewed")]
    ChangedSincePreview,
    #[error("configuration takeover is unsafe while UPS power state is {0}")]
    UnsafePowerState(String),
    #[error(
        "the remote NUT mode belongs to the opposite role; use the explicit role transition workflow"
    )]
    RoleTransitionRequired,
    #[error("remote configuration inspection failed: {0}")]
    Inspection(String),
    #[error("configuration apply failed and original files were restored: {0}")]
    ApplyFailed(String),
    #[error("configuration apply failed and rollback also failed: {0}")]
    RollbackFailed(String),
    #[error("local backup failed: {0}")]
    Backup(#[from] std::io::Error),
    #[error("stored configuration manifest is invalid: {0}")]
    InvalidManifest(#[from] serde_json::Error),
}

pub fn server_candidate(
    server: &ServerRecord,
    credentials: &[CredentialRecord],
) -> BTreeMap<String, String> {
    let primary = credentials
        .iter()
        .find(|credential| credential.username == "nwm_primary")
        .expect("a server always has its primary credential");
    let mut files = BTreeMap::new();
    files.insert("/etc/nut/nut.conf".into(), "MODE=netserver\n".into());

    let mut ups = format!(
        "# Keep the Debian/NUT packaged default for slow USB devices.\nmaxretry = 3\n\n[{}]\n    driver = {}\n    port = {}\n    desc = \"NUT Web Manager USB UPS\"\n",
        server.ups_name,
        quote_if_needed(&server.device.driver),
        quote_if_needed(&server.device.port),
    );
    for (key, value) in &server.device.selectors {
        if matches!(key.as_str(), "driver" | "port")
            || !is_durable_usb_selector(key)
            || !valid_directive(key)
        {
            continue;
        }
        ups.push_str(&format!("    {key} = \"{}\"\n", escape_value(value)));
    }
    ups.push_str(&format!(
        "    # NWM-managed low-battery trigger and emergency fallback; do not rely on firmware defaults.\n    ignorelb\n    override.battery.charge.low = {}\n    override.battery.runtime.low = -1\n",
        server.shutdown.battery_level_percent
    ));
    if server.shutdown.powerdown_enabled {
        ups.push_str(
            "    # Explicitly unlock the driver's late-shutdown killpower operation.\n    allow_killpower\n",
        );
    }
    files.insert("/etc/nut/ups.conf".into(), ups);
    files.insert(
        "/etc/nut/upsd.conf".into(),
        format!("LISTEN 0.0.0.0 {}\n", server.listen_port),
    );

    let mut users = String::new();
    for credential in credentials {
        let role = if credential.username == "nwm_primary" {
            "primary"
        } else {
            "secondary"
        };
        users.push_str(&format!(
            "[{}]\n    password = {}\n    upsmon {}\n\n",
            credential.username, credential.secret, role
        ));
    }
    files.insert("/etc/nut/upsd.users".into(), users);
    let (shutdown_command, powerdown) = if server.shutdown.powerdown_enabled {
        ("/sbin/shutdown -h +0", "POWERDOWNFLAG /etc/killpower\n")
    } else {
        (
            "/etc/nut/nwm-shutdown-command",
            "# The shutdown wrapper removes this flag before systemd reaches Debian's nutshutdown hook.\nPOWERDOWNFLAG /run/nut/nwm-powerdown-disabled\n",
        )
    };
    let scheduling = match server.shutdown.trigger_mode {
        ShutdownTriggerMode::BatteryLevel => String::new(),
        ShutdownTriggerMode::OnBatteryTimer => {
            "NOTIFYCMD /usr/sbin/upssched\nNOTIFYFLAG ONBATT SYSLOG+EXEC\nNOTIFYFLAG ONLINE SYSLOG+EXEC\n".into()
        }
    };
    files.insert(
        "/etc/nut/upsmon.conf".into(),
        format!(
            "MONITOR {}@localhost 1 {} {} primary\nMINSUPPLIES 1\nSHUTDOWNCMD \"{shutdown_command}\"\n{powerdown}HOSTSYNC {}\nFINALDELAY {}\n{scheduling}",
            server.ups_name,
            primary.username,
            primary.secret,
            server.shutdown.host_sync_seconds,
            server.shutdown.final_delay_seconds,
        ),
    );
    files.insert(
        "/etc/nut/nwm-shutdown-command".into(),
        if server.shutdown.powerdown_enabled {
            "#!/bin/sh\nexec /sbin/shutdown -h +0\n".into()
        } else {
            "#!/bin/sh\nset -eu\nflag=/run/nut/nwm-powerdown-disabled\nrm -f \"$flag\"\nif /usr/sbin/upsmon -K >/dev/null 2>&1; then\n    echo 'NWM refused shutdown because the NUT killpower flag is still active' >&2\n    exit 1\nfi\nexec /sbin/shutdown -h +0\n".into()
        },
    );
    let upssched = match server.shutdown.trigger_mode {
        ShutdownTriggerMode::BatteryLevel => {
            "# Disabled: NWM is using the battery-level shutdown strategy.\n".into()
        }
        ShutdownTriggerMode::OnBatteryTimer => format!(
            "CMDSCRIPT /etc/nut/nwm-upssched-command\nPIPEFN /run/nut/nwm-upssched.pipe\nLOCKFN /run/nut/nwm-upssched.lock\nAT ONBATT * START-TIMER nwm-on-battery-expired {}\nAT ONLINE * CANCEL-TIMER nwm-on-battery-expired\n",
            server.shutdown.on_battery_seconds
        ),
    };
    files.insert("/etc/nut/upssched.conf".into(), upssched);
    files.insert(
        "/etc/nut/nwm-upssched-command".into(),
        "#!/bin/sh\nset -eu\ncase \"${1:-}\" in\n    nwm-on-battery-expired) exec /usr/sbin/upsmon -c fsd ;;\n    *) exit 0 ;;\nesac\n".into(),
    );
    files
}

pub fn client_candidate(
    server: &ServerRecord,
    server_host: &Host,
    credential: &CredentialRecord,
) -> BTreeMap<String, String> {
    let address = if server_host.address.contains(':') && !server_host.address.starts_with('[') {
        format!("[{}]", server_host.address)
    } else {
        server_host.address.clone()
    };
    BTreeMap::from([
        ("/etc/nut/nut.conf".into(), "MODE=netclient\n".into()),
        (
            "/etc/nut/upsmon.conf".into(),
            format!(
                "MONITOR {}@{}:{} 1 {} {} secondary\nMINSUPPLIES 1\nSHUTDOWNCMD \"/sbin/shutdown -h +0\"\nHOSTSYNC 15\nFINALDELAY 5\n",
                server.ups_name,
                address,
                server.listen_port,
                credential.username,
                credential.secret,
            ),
        ),
    ])
}

pub async fn preview(
    ssh: &SshManager,
    host: &Host,
    candidate: &BTreeMap<String, String>,
    latest: Option<&RevisionRecord>,
) -> Result<ConfigPreview, ConfigError> {
    let snapshot = inspect(ssh, host, candidate.keys().map(String::as_str)).await?;
    let assessment = assess_ownership(host.role, &snapshot, latest)?;
    let takeover_required = !assessment.conflicts.is_empty();
    let role_transition_required = has_role_mismatch(host.role, &snapshot);
    Ok(ConfigPreview {
        role: host.role,
        ownership: assessment.ownership,
        files: snapshot
            .iter()
            .map(|file| ConfigPreviewFile {
                path: file.path.clone(),
                current: file.content.clone(),
                candidate: candidate
                    .get(&file.path)
                    .expect("candidate path inspected")
                    .clone(),
            })
            .collect(),
        services: services(host.role),
        conflicts: assessment.conflicts,
        takeover_required,
        snapshot_hash: snapshot_hash(&snapshot),
        takeover_allowed: !takeover_required,
        takeover_block_reason: if role_transition_required {
            Some("the remote NUT mode belongs to the opposite role".into())
        } else {
            takeover_required.then(|| "UPS power state has not been checked".to_owned())
        },
        takeover_warning: None,
        role_transition_required,
    })
}

pub fn validate_preview(
    preview: &ConfigPreview,
    takeover_snapshot: Option<&str>,
) -> Result<bool, ConfigError> {
    if let Some(expected) = takeover_snapshot
        && preview.snapshot_hash != expected
    {
        return Err(ConfigError::ChangedSincePreview);
    }
    let Some(conflict) = preview.conflicts.first() else {
        return Ok(false);
    };
    if takeover_snapshot.is_some() && preview.role_transition_required {
        return Err(ConfigError::RoleTransitionRequired);
    }
    if takeover_snapshot.is_none() {
        return Err(ConfigError::Conflict {
            path: conflict.path.clone(),
            reason: conflict.reason.clone(),
        });
    }
    Ok(true)
}

pub async fn require_safe_power_state(
    ssh: &SshManager,
    server_host: &Host,
) -> Result<TakeoverPowerCheck, ConfigError> {
    let script = r#"set -u
found=0
for ups_name in $(upsc -l 2>/dev/null || true); do
    status=$(upsc "$ups_name@localhost" ups.status 2>/dev/null || true)
    [ -n "$status" ] || continue
    found=1
    printf 'NWM_UPS_STATUS\t%s\t%s\n' "$ups_name" "$status"
done
[ "$found" -eq 1 ] || printf 'NWM_UPS_STATUS\t-\tUNKNOWN\n'
"#;
    let output = ssh
        .execute_script(server_host, script, Duration::from_secs(15))
        .await?;
    let statuses: Vec<_> = output
        .lines()
        .filter_map(|line| {
            let mut fields = line.splitn(3, '\t');
            (fields.next() == Some("NWM_UPS_STATUS")).then(|| {
                (
                    fields.next().unwrap_or("-"),
                    fields.next().unwrap_or("UNKNOWN"),
                )
            })
        })
        .collect();
    validate_power_statuses(&statuses)
}

fn validate_power_statuses(statuses: &[(&str, &str)]) -> Result<TakeoverPowerCheck, ConfigError> {
    if statuses.is_empty() || (statuses.len() == 1 && statuses[0] == ("-", "UNKNOWN")) {
        return Ok(TakeoverPowerCheck {
            summary: "-:UNKNOWN".into(),
            warning: Some(
                "当前 NUT 驱动未连接，无法确认 UPS 是否处于市电供电状态；仅应在确认现场市电正常时执行恢复接管"
                    .into(),
            ),
        });
    }
    let mut summary = Vec::new();
    for (name, status) in statuses {
        summary.push(format!("{name}:{status}"));
        let flags: Vec<_> = status.split_whitespace().collect();
        if flags
            .iter()
            .any(|flag| matches!(*flag, "OB" | "LB" | "FSD"))
            || !flags.contains(&"OL")
        {
            return Err(ConfigError::UnsafePowerState(summary.join(", ")));
        }
    }
    Ok(TakeoverPowerCheck {
        summary: summary.join(", "),
        warning: None,
    })
}

pub async fn apply(
    ssh: &SshManager,
    data_dir: &Path,
    host: &Host,
    candidate: &BTreeMap<String, String>,
    latest: Option<&RevisionRecord>,
    server: Option<&ServerRecord>,
    takeover_snapshot: Option<&str>,
) -> Result<ConfigApplyOutcome, ConfigError> {
    let snapshot = inspect(ssh, host, candidate.keys().map(String::as_str)).await?;
    let assessment = assess_ownership(host.role, &snapshot, latest)?;
    let takeover = authorize_apply(&assessment, &snapshot, takeover_snapshot)?;
    let backup_path = write_backup(data_dir, host, &snapshot)?;
    let script = apply_script(host.role, candidate, &snapshot, server);
    let output = ssh.execute_script(host, &script, CONFIG_TIMEOUT).await?;
    let local_validation = match output.lines().next().unwrap_or_default() {
        "NWM_APPLY=success" => output.lines().skip(1).collect::<Vec<_>>().join("\n"),
        "NWM_APPLY=rolled_back" => {
            return Err(ConfigError::ApplyFailed(
                output.lines().skip(1).collect::<Vec<_>>().join("\n"),
            ));
        }
        "NWM_APPLY=rollback_failed" => {
            return Err(ConfigError::RollbackFailed(
                output.lines().skip(1).collect::<Vec<_>>().join("\n"),
            ));
        }
        _ => return Err(ConfigError::Inspection(truncate(&output))),
    };
    let manifest = ConfigManifest {
        files: candidate
            .iter()
            .map(|(path, content)| (path.clone(), hash(content)))
            .collect(),
    };
    let manifest_json = serde_json::to_string(&manifest)?;
    let (tcp_reachable, warning) = if let Some(server) = server {
        match query_ups(&host.address, server.listen_port, &server.ups_name).await {
            Ok(_) => (Some(true), None),
            Err(error) => (
                Some(false),
                Some(format!(
                    "NUT works locally, but TCP {}:{} is not reachable from NWM: {error}. Check the LAN/firewall manually.",
                    host.address, server.listen_port
                )),
            ),
        }
    } else {
        (None, None)
    };
    Ok(ConfigApplyOutcome {
        backup_path: backup_path.to_string_lossy().into_owned(),
        manifest_json,
        local_validation,
        tcp_reachable,
        warning,
        takeover,
    })
}

pub async fn restore_backup(
    ssh: &SshManager,
    host: &Host,
    backup_path: &Path,
) -> Result<(), ConfigError> {
    let metadata = fs::read(backup_path.join("metadata.json"))?;
    let snapshot: Vec<RemoteFile> = serde_json::from_slice(&metadata)?;
    let mut script = String::from("#!/bin/sh\nset -u\nfailed=''\n");
    for file in &snapshot {
        if file.exists {
            script.push_str(&format!(
                "printf %s '{}' | base64 -d > '{}' || failed='restore {}'\nchown {}:{} '{}' || failed='owner {}'\nchmod {} '{}' || failed='mode {}'\n",
                STANDARD.encode(&file.content), file.path, file.path,
                file.uid, file.gid, file.path, file.path, file.mode, file.path, file.path,
            ));
        } else {
            script.push_str(&format!(
                "rm -f '{}' || failed='remove {}'\n",
                file.path, file.path
            ));
        }
    }
    match host.role {
        HostRole::Server => script.push_str(
            "pkill -TERM -x upssched >/dev/null 2>&1 || true\nrm -f /run/nut/nwm-upssched.pipe /run/nut/nwm-upssched.lock >/dev/null 2>&1 || true\nsystemctl daemon-reload >/dev/null 2>&1 || true\nsystemctl start nut-driver-enumerator.service >/dev/null 2>&1 || true\nsystemctl restart nut-driver.target nut-server.service nut-monitor.service >/dev/null 2>&1 || true\n",
        ),
        HostRole::Client => script.push_str(
            "systemctl daemon-reload >/dev/null 2>&1 || true\nsystemctl restart nut-monitor.service >/dev/null 2>&1 || true\n",
        ),
    }
    script.push_str("[ -z \"$failed\" ] || { printf '%s\\n' \"$failed\"; exit 1; }\nprintf '%s\\n' 'NWM_RESTORE=success'\n");
    ssh.execute_script(host, &script, CONFIG_TIMEOUT)
        .await
        .map(|_| ())
        .map_err(ConfigError::Ssh)
}

async fn inspect<'a>(
    ssh: &SshManager,
    host: &Host,
    paths: impl Iterator<Item = &'a str>,
) -> Result<Vec<RemoteFile>, ConfigError> {
    let paths: Vec<_> = paths.collect();
    let mut script = String::from("set -eu\n");
    for path in &paths {
        script.push_str(&format!(
            "if [ -e {path} ]; then printf 'NWM_FILE\\t%s\\t1\\t%s\\t%s\\t%s\\t' {path} \"$(stat -c %a {path})\" \"$(stat -c %u {path})\" \"$(stat -c %g {path})\"; base64 -w0 {path}; printf '\\n'; else printf 'NWM_FILE\\t%s\\t0\\t640\\t0\\t0\\t\\n' {path}; fi\n"
        ));
    }
    let output = ssh
        .execute_script(host, &script, Duration::from_secs(15))
        .await?;
    let mut files = Vec::new();
    for line in output.lines() {
        let fields: Vec<_> = line.splitn(7, '\t').collect();
        if fields.len() != 7 || fields[0] != "NWM_FILE" {
            return Err(ConfigError::Inspection(truncate(line)));
        }
        let content = STANDARD
            .decode(fields[6])
            .map_err(|error| ConfigError::Inspection(error.to_string()))?;
        files.push(RemoteFile {
            path: fields[1].into(),
            exists: fields[2] == "1",
            mode: fields[3].into(),
            uid: fields[4].into(),
            gid: fields[5].into(),
            content: String::from_utf8(content)
                .map_err(|error| ConfigError::Inspection(error.to_string()))?,
        });
    }
    if files.len() != paths.len() {
        return Err(ConfigError::Inspection(
            "not all requested files were returned".into(),
        ));
    }
    Ok(files)
}

#[derive(Debug)]
struct OwnershipAssessment {
    ownership: ConfigOwnership,
    conflicts: Vec<ConfigConflict>,
}

fn assess_ownership(
    role: HostRole,
    snapshot: &[RemoteFile],
    latest: Option<&RevisionRecord>,
) -> Result<OwnershipAssessment, ConfigError> {
    if let Some(latest) = latest {
        let manifest: ConfigManifest = serde_json::from_str(&latest.manifest_json)?;
        let mut conflicts = Vec::new();
        for file in snapshot {
            let Some(expected) = manifest.files.get(&file.path) else {
                conflicts.push(ConfigConflict {
                    path: file.path.clone(),
                    reason: "the previous managed revision did not contain this file".into(),
                });
                continue;
            };
            if &hash(&file.content) != expected {
                conflicts.push(ConfigConflict {
                    path: file.path.clone(),
                    reason: "the file changed after the last successful NWM apply".into(),
                });
            }
        }
        return Ok(OwnershipAssessment {
            ownership: if conflicts.is_empty() {
                ConfigOwnership::ManagedUnchanged
            } else {
                ConfigOwnership::ManagedModified
            },
            conflicts,
        });
    }
    let mut conflicts = Vec::new();
    for file in snapshot {
        if !distribution_default(role, &file.path, &file.content) {
            conflicts.push(ConfigConflict {
                path: file.path.clone(),
                reason: "the host already has an effective NUT configuration".into(),
            });
        }
    }
    Ok(OwnershipAssessment {
        ownership: if conflicts.is_empty() {
            ConfigOwnership::DistributionDefault
        } else {
            ConfigOwnership::UnmanagedExisting
        },
        conflicts,
    })
}

fn authorize_apply(
    assessment: &OwnershipAssessment,
    snapshot: &[RemoteFile],
    takeover_snapshot: Option<&str>,
) -> Result<bool, ConfigError> {
    if let Some(expected) = takeover_snapshot
        && snapshot_hash(snapshot) != expected
    {
        return Err(ConfigError::ChangedSincePreview);
    }
    let Some(conflict) = assessment.conflicts.first() else {
        return Ok(false);
    };
    if takeover_snapshot.is_none() {
        return Err(ConfigError::Conflict {
            path: conflict.path.clone(),
            reason: conflict.reason.clone(),
        });
    }
    Ok(true)
}

fn snapshot_hash(snapshot: &[RemoteFile]) -> String {
    let encoded = serde_json::to_vec(snapshot).expect("remote snapshot is serializable");
    format!("{:x}", Sha256::digest(encoded))
}

fn distribution_default(role: HostRole, path: &str, content: &str) -> bool {
    let effective: Vec<_> = content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    match path {
        "/etc/nut/nut.conf" => {
            effective.is_empty()
                || effective.iter().all(|line| {
                    let compact = line.replace(' ', "");
                    compact.eq_ignore_ascii_case("MODE=none")
                })
        }
        "/etc/nut/ups.conf" => {
            effective.is_empty()
                || effective.iter().all(|line| {
                    line.replace([' ', '\t'], "")
                        .eq_ignore_ascii_case("maxretry=3")
                })
        }
        "/etc/nut/upsd.conf" => !effective
            .iter()
            .any(|line| starts_directive(line, "LISTEN")),
        "/etc/nut/upsd.users" => !effective.iter().any(|line| line.starts_with('[')),
        "/etc/nut/upsmon.conf" => !effective
            .iter()
            .any(|line| starts_directive(line, "MONITOR")),
        "/etc/nut/upssched.conf"
        | "/etc/nut/nwm-upssched-command"
        | "/etc/nut/nwm-shutdown-command" => effective.is_empty(),
        _ => role == HostRole::Client && effective.is_empty(),
    }
}

fn has_role_mismatch(role: HostRole, snapshot: &[RemoteFile]) -> bool {
    let Some(file) = snapshot
        .iter()
        .find(|file| file.path == "/etc/nut/nut.conf")
    else {
        return false;
    };
    let mode = file.content.lines().find_map(|line| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        let (key, value) = line.split_once('=')?;
        key.trim()
            .eq_ignore_ascii_case("MODE")
            .then(|| value.trim().to_ascii_lowercase())
    });
    !matches!(
        (role, mode.as_deref()),
        (_, None | Some("none"))
            | (HostRole::Server, Some("netserver" | "standalone"))
            | (HostRole::Client, Some("netclient"))
    )
}

fn apply_script(
    role: HostRole,
    candidate: &BTreeMap<String, String>,
    snapshot: &[RemoteFile],
    server: Option<&ServerRecord>,
) -> String {
    let token = uuid::Uuid::new_v4().simple().to_string();
    let mut script = String::from("#!/bin/sh\nset -u\nfailed=''\n");
    for (path, content) in candidate {
        let name = Path::new(path).file_name().unwrap().to_string_lossy();
        let temp = format!("/etc/nut/.nwm-{token}-{name}");
        script.push_str(&format!(
            "printf %s '{}' | base64 -d > '{}' || failed='write {}'\nchown root:nut '{}' 2>/dev/null || chown root:root '{}' || failed='chown {}'\nchmod {} '{}' || failed='chmod {}'\n",
            STANDARD.encode(content), temp, path, temp, temp, path,
            candidate_mode(path), temp, path,
        ));
    }
    script.push_str("if [ -z \"$failed\" ]; then\n");
    for path in candidate.keys() {
        let name = Path::new(path).file_name().unwrap().to_string_lossy();
        script.push_str(&format!(
            "mv '/etc/nut/.nwm-{token}-{name}' '{path}' || failed='replace {path}'\n"
        ));
    }
    script.push_str("fi\n");
    let validate = match role {
        HostRole::Server => {
            let server = server.expect("server validation requires server model");
            let usb_permissions = usb_permission_refresh(server);
            let scheduler_validation = if server.shutdown.trigger_mode
                == ShutdownTriggerMode::OnBatteryTimer
            {
                "[ -n \"$failed\" ] || command -v upssched >/dev/null 2>&1 || failed='upssched missing'\n[ -n \"$failed\" ] || test -x /etc/nut/nwm-upssched-command || failed='upssched command not executable'\n[ -n \"$failed\" ] || sh -n /etc/nut/nwm-upssched-command || failed='upssched command invalid'\n"
            } else {
                ""
            };
            let shutdown_validation = "[ -n \"$failed\" ] || test -x /etc/nut/nwm-shutdown-command || failed='shutdown command not executable'\n[ -n \"$failed\" ] || sh -n /etc/nut/nwm-shutdown-command || failed='shutdown command invalid'\n";
            format!(
                "systemctl daemon-reload || failed='systemd daemon-reload'\n{usb_permissions}{scheduler_validation}{shutdown_validation}pkill -TERM -x upssched >/dev/null 2>&1 || true\nrm -f /run/nut/nwm-upssched.pipe /run/nut/nwm-upssched.lock >/dev/null 2>&1 || true\n[ -n \"$failed\" ] || systemctl start nut-driver-enumerator.service || failed='nut-driver enumeration'\n[ -n \"$failed\" ] || systemctl restart nut-driver@{}.service || failed='nut-driver restart'\n[ -n \"$failed\" ] || systemctl restart nut-server.service || failed='nut-server restart'\n[ -n \"$failed\" ] || systemctl restart nut-monitor.service || failed='nut-monitor restart'\n[ -n \"$failed\" ] || systemctl is-active --quiet nut-driver@{}.service || failed='nut-driver inactive'\n[ -n \"$failed\" ] || systemctl is-active --quiet nut-server.service || failed='nut-server inactive'\n[ -n \"$failed\" ] || systemctl is-active --quiet nut-monitor.service || failed='nut-monitor inactive'\n[ -n \"$failed\" ] || ss -ltn | grep -Eq '[:.]{}[[:space:]]' || failed='TCP 3493 listener missing'\n[ -n \"$failed\" ] || upsc '{}@localhost' >/dev/null 2>&1 || failed='local upsc failed'\n[ -n \"$failed\" ] || systemctl enable nut-driver-enumerator.path nut-driver.target nut-server.service nut-monitor.service >/dev/null 2>&1 || failed='enable NUT services'\n",
                server.ups_name, server.ups_name, server.listen_port, server.ups_name,
            )
        }
        HostRole::Client => {
            let monitor = candidate.get("/etc/nut/upsmon.conf").unwrap();
            let endpoint = monitor
                .lines()
                .find(|line| line.starts_with("MONITOR "))
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap();
            format!(
                "systemctl daemon-reload || failed='systemd daemon-reload'\n[ -n \"$failed\" ] || systemctl restart nut-monitor.service || failed='nut-monitor restart'\n[ -n \"$failed\" ] || systemctl is-active --quiet nut-monitor.service || failed='nut-monitor inactive'\n[ -n \"$failed\" ] || upsc '{}' >/dev/null 2>&1 || failed='remote upsc failed'\n[ -n \"$failed\" ] || systemctl enable nut-monitor.service >/dev/null 2>&1 || failed='enable nut-monitor'\n",
                endpoint,
            )
        }
    };
    script.push_str(&validate);
    script.push_str("if [ -z \"$failed\" ]; then printf '%s\\n' 'NWM_APPLY=success'; printf '%s\\n' 'NUT configuration and services verified'; exit 0; fi\nrollback_failed=''\n");
    for file in snapshot {
        if file.exists {
            script.push_str(&format!(
                "printf %s '{}' | base64 -d > '{}' || rollback_failed='restore {}'\nchown {}:{} '{}' || rollback_failed='owner {}'\nchmod {} '{}' || rollback_failed='mode {}'\n",
                STANDARD.encode(&file.content), file.path, file.path,
                file.uid, file.gid, file.path, file.path, file.mode, file.path, file.path,
            ));
        } else {
            script.push_str(&format!(
                "rm -f '{}' || rollback_failed='remove {}'\n",
                file.path, file.path
            ));
        }
    }
    match role {
        HostRole::Server => script.push_str("pkill -TERM -x upssched >/dev/null 2>&1 || true\nrm -f /run/nut/nwm-upssched.pipe /run/nut/nwm-upssched.lock >/dev/null 2>&1 || true\nsystemctl start nut-driver-enumerator.service >/dev/null 2>&1 || true\nsystemctl restart nut-driver.target nut-server.service nut-monitor.service >/dev/null 2>&1 || true\n"),
        HostRole::Client => script.push_str("systemctl restart nut-monitor.service >/dev/null 2>&1 || true\n"),
    }
    for path in candidate.keys() {
        let name = Path::new(path).file_name().unwrap().to_string_lossy();
        script.push_str(&format!(
            "rm -f '/etc/nut/.nwm-{token}-{name}' >/dev/null 2>&1 || true\n"
        ));
    }
    script.push_str("if [ -z \"$rollback_failed\" ]; then printf '%s\\n' 'NWM_APPLY=rolled_back'; printf '%s\\n' \"$failed\"; else printf '%s\\n' 'NWM_APPLY=rollback_failed'; printf '%s; %s\\n' \"$failed\" \"$rollback_failed\"; fi\n");
    script
}

fn write_backup(
    data_dir: &Path,
    host: &Host,
    snapshot: &[RemoteFile],
) -> Result<PathBuf, std::io::Error> {
    let directory = data_dir
        .join("backups")
        .join(host.id.to_string())
        .join(format!(
            "{}-{}",
            Utc::now().format("%Y%m%dT%H%M%SZ"),
            uuid::Uuid::new_v4().simple()
        ));
    fs::create_dir_all(&directory)?;
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
    for file in snapshot {
        if file.exists {
            let name = Path::new(&file.path).file_name().unwrap();
            fs::write(directory.join(name), &file.content)?;
        }
    }
    fs::write(
        directory.join("metadata.json"),
        serde_json::to_vec_pretty(snapshot).map_err(std::io::Error::other)?,
    )?;
    Ok(directory)
}

pub async fn query_ups(address: &str, port: u16, ups_name: &str) -> Result<String, String> {
    let destination = if address.contains(':') && !address.starts_with('[') {
        format!("[{address}]:{port}")
    } else {
        format!("{address}:{port}")
    };
    let stream = timeout(NETWORK_TIMEOUT, TcpStream::connect(&destination))
        .await
        .map_err(|_| "connection timed out".to_owned())?
        .map_err(|error| error.to_string())?;
    let mut stream = BufReader::new(stream);
    stream
        .get_mut()
        .write_all(format!("GET VAR {ups_name} ups.status\n").as_bytes())
        .await
        .map_err(|error| error.to_string())?;
    let mut response = String::new();
    timeout(NETWORK_TIMEOUT, stream.read_line(&mut response))
        .await
        .map_err(|_| "response timed out".to_owned())?
        .map_err(|error| error.to_string())?;
    if response.starts_with("VAR ") {
        Ok(response.trim().to_owned())
    } else {
        Err(format!("unexpected upsd response: {}", response.trim()))
    }
}

pub async fn query_ups_variables(
    address: &str,
    port: u16,
    ups_name: &str,
) -> Result<std::collections::HashMap<String, String>, String> {
    let destination = if address.contains(':') && !address.starts_with('[') {
        format!("[{address}]:{port}")
    } else {
        format!("{address}:{port}")
    };
    let stream = timeout(NETWORK_TIMEOUT, TcpStream::connect(&destination))
        .await
        .map_err(|_| "connection timed out".to_owned())?
        .map_err(|error| error.to_string())?;
    let mut stream = BufReader::new(stream);
    stream
        .get_mut()
        .write_all(format!("LIST VAR {ups_name}\n").as_bytes())
        .await
        .map_err(|error| error.to_string())?;

    let mut variables = std::collections::HashMap::new();
    loop {
        let mut response = String::new();
        let bytes = timeout(NETWORK_TIMEOUT, stream.read_line(&mut response))
            .await
            .map_err(|_| "response timed out".to_owned())?
            .map_err(|error| error.to_string())?;
        if bytes == 0 {
            return Err("upsd closed the connection before completing the response".into());
        }
        let line = response.trim();
        if line == format!("END LIST VAR {ups_name}") {
            break;
        }
        if line.starts_with("ERR ") {
            return Err(format!("upsd returned: {line}"));
        }
        let Some(rest) = line.strip_prefix(&format!("VAR {ups_name} ")) else {
            continue;
        };
        let Some((key, encoded)) = rest.split_once(' ') else {
            continue;
        };
        variables.insert(key.to_owned(), decode_upsd_value(encoded));
    }
    if variables.is_empty() {
        Err("upsd returned no UPS variables".into())
    } else {
        Ok(variables)
    }
}

fn decode_upsd_value(value: &str) -> String {
    let value = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value);
    let mut decoded = String::with_capacity(value.len());
    let mut escaped = false;
    for character in value.chars() {
        if escaped {
            decoded.push(character);
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else {
            decoded.push(character);
        }
    }
    if escaped {
        decoded.push('\\');
    }
    decoded
}

fn services(role: HostRole) -> Vec<String> {
    match role {
        HostRole::Server => vec![
            "nut-driver@<ups>".into(),
            "nut-server".into(),
            "nut-monitor".into(),
        ],
        HostRole::Client => vec!["nut-monitor".into()],
    }
}

fn candidate_mode(path: &str) -> &'static str {
    if path.ends_with("nut.conf") {
        "644"
    } else if path.ends_with("-command") {
        "750"
    } else {
        "640"
    }
}

fn usb_permission_refresh(server: &ServerRecord) -> String {
    let vendor_id = usb_id(server.device.selectors.get("vendorid"));
    let product_id = usb_id(server.device.selectors.get("productid"));
    let trigger = match (vendor_id, product_id) {
        (Some(vendor_id), Some(product_id)) => format!(
            "udevadm trigger --action=change --subsystem-match=usb --attr-match=idVendor={vendor_id} --attr-match=idProduct={product_id}"
        ),
        _ => "udevadm trigger --action=change --subsystem-match=usb".into(),
    };
    format!(
        "[ -n \"$failed\" ] || udevadm control --reload-rules || failed='reload NUT USB permissions'\n[ -n \"$failed\" ] || {trigger} || failed='apply NUT USB permissions'\n[ -n \"$failed\" ] || udevadm settle --timeout=10 || failed='wait for NUT USB permissions'\n"
    )
}

fn usb_id(value: Option<&String>) -> Option<String> {
    value
        .filter(|value| {
            value.len() == 4 && value.chars().all(|character| character.is_ascii_hexdigit())
        })
        .map(|value| value.to_ascii_lowercase())
}

fn starts_directive(line: &str, directive: &str) -> bool {
    line.split_whitespace()
        .next()
        .is_some_and(|word| word.eq_ignore_ascii_case(directive))
}

fn hash(content: &str) -> String {
    format!("{:x}", Sha256::digest(content.as_bytes()))
}

fn valid_directive(value: &str) -> bool {
    !value.is_empty()
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
        })
}

fn quote_if_needed(value: &str) -> String {
    if value.chars().all(|character| {
        character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-' | '/')
    }) {
        value.to_owned()
    } else {
        format!("\"{}\"", escape_value(value))
    }
}

fn escape_value(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace(['\n', '\r'], " ")
}

fn truncate(value: &str) -> String {
    value.chars().take(1500).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nwm_common::{ApplyState, ConfigRevisionId, HostId, NutServerId, UpsId};

    fn remote_file(path: &str, content: &str) -> RemoteFile {
        RemoteFile {
            path: path.into(),
            exists: true,
            mode: "640".into(),
            uid: "0".into(),
            gid: "0".into(),
            content: content.into(),
        }
    }

    #[test]
    fn default_detection_accepts_debian_defaults_but_rejects_monitor() {
        assert!(distribution_default(
            HostRole::Server,
            "/etc/nut/nut.conf",
            "# comment\nMODE=none\n"
        ));
        assert!(distribution_default(
            HostRole::Server,
            "/etc/nut/ups.conf",
            "# Set maxretry to 3 by default\nmaxretry = 3\n"
        ));
        assert!(!distribution_default(
            HostRole::Server,
            "/etc/nut/ups.conf",
            "maxretry = 3\n[existing]\ndriver = usbhid-ups\nport = auto\n"
        ));
        assert!(distribution_default(
            HostRole::Server,
            "/etc/nut/upsmon.conf",
            "MINSUPPLIES 1\nSHUTDOWNCMD /sbin/poweroff\n"
        ));
        assert!(!distribution_default(
            HostRole::Client,
            "/etc/nut/upsmon.conf",
            "MONITOR ups@host 1 u p secondary\n"
        ));
    }

    #[test]
    fn existing_and_modified_configuration_require_explicit_takeover() {
        let existing = vec![remote_file("/etc/nut/nut.conf", "MODE=netserver\n")];
        let unmanaged = assess_ownership(HostRole::Server, &existing, None).unwrap();
        assert_eq!(unmanaged.ownership, ConfigOwnership::UnmanagedExisting);
        assert_eq!(unmanaged.conflicts.len(), 1);
        assert!(matches!(
            authorize_apply(&unmanaged, &existing, None),
            Err(ConfigError::Conflict { .. })
        ));
        let preview_hash = snapshot_hash(&existing);
        assert!(authorize_apply(&unmanaged, &existing, Some(&preview_hash)).unwrap());

        let revision = RevisionRecord {
            id: ConfigRevisionId::new(),
            revision_number: 1,
            manifest_json: serde_json::to_string(&ConfigManifest {
                files: BTreeMap::from([("/etc/nut/nut.conf".into(), hash("MODE=netclient\n"))]),
            })
            .unwrap(),
            backup_path: "/tmp/backup".into(),
        };
        let modified = assess_ownership(HostRole::Server, &existing, Some(&revision)).unwrap();
        assert_eq!(modified.ownership, ConfigOwnership::ManagedModified);
        assert_eq!(modified.conflicts.len(), 1);
    }

    #[test]
    fn takeover_rejects_a_stale_snapshot() {
        let snapshot = vec![remote_file("/etc/nut/nut.conf", "MODE=netserver\n")];
        let assessment = assess_ownership(HostRole::Server, &snapshot, None).unwrap();
        assert!(matches!(
            authorize_apply(&assessment, &snapshot, Some("stale")),
            Err(ConfigError::ChangedSincePreview)
        ));
    }

    #[test]
    fn opposite_remote_mode_requires_role_transition() {
        let client_mode = vec![remote_file("/etc/nut/nut.conf", "MODE=netclient\n")];
        let server_mode = vec![remote_file("/etc/nut/nut.conf", "MODE=netserver\n")];
        assert!(has_role_mismatch(HostRole::Server, &client_mode));
        assert!(has_role_mismatch(HostRole::Client, &server_mode));
        assert!(!has_role_mismatch(HostRole::Server, &server_mode));
        assert!(!has_role_mismatch(HostRole::Client, &client_mode));
    }

    #[test]
    fn takeover_blocks_known_unsafe_power_states_but_allows_driver_repair() {
        assert_eq!(
            validate_power_statuses(&[("ups", "OL CHRG")])
                .unwrap()
                .summary,
            "ups:OL CHRG"
        );
        for status in ["OB", "OB LB", "FSD", "UNKNOWN", "CAL"] {
            assert!(matches!(
                validate_power_statuses(&[("ups", status)]),
                Err(ConfigError::UnsafePowerState(_))
            ));
        }
        assert!(validate_power_statuses(&[]).unwrap().warning.is_some());
        assert!(
            validate_power_statuses(&[("-", "UNKNOWN")])
                .unwrap()
                .warning
                .is_some()
        );
    }

    #[test]
    fn server_generation_uses_scanner_values_and_real_credentials() {
        let mut server = ServerRecord {
            id: NutServerId::new(),
            host_id: HostId::new(),
            ups_name: "ups".into(),
            listen_address: "0.0.0.0".into(),
            listen_port: 3493,
            enabled: false,
            apply_state: ApplyState::Unconfigured,
            applied_revision_id: None,
            shutdown: crate::persistence::ShutdownOptions {
                trigger_mode: ShutdownTriggerMode::BatteryLevel,
                battery_level_percent: 20,
                on_battery_seconds: 300,
                host_sync_seconds: 15,
                final_delay_seconds: 5,
                powerdown_enabled: true,
            },
            device: crate::persistence::DeviceRecord {
                id: UpsId::new(),
                name: "ups".into(),
                driver: "usbhid-ups".into(),
                port: "auto".into(),
                selectors: BTreeMap::from([
                    ("vendorid".into(), "051d".into()),
                    ("productid".into(), "0002".into()),
                    ("serial".into(), "ABC".into()),
                    ("bus".into(), "002".into()),
                    ("device".into(), "004".into()),
                    ("busport".into(), "001".into()),
                ]),
            },
        };
        let credentials = vec![
            CredentialRecord {
                id: nwm_common::CredentialId::new(),
                username: "nwm_primary".into(),
                secret: "secret".into(),
            },
            CredentialRecord {
                id: nwm_common::CredentialId::new(),
                username: "nwm".into(),
                secret: "nwm".into(),
            },
        ];
        let files = server_candidate(&server, &credentials);
        assert!(files["/etc/nut/ups.conf"].contains("vendorid = \"051d\""));
        assert!(files["/etc/nut/ups.conf"].contains("productid = \"0002\""));
        assert!(files["/etc/nut/ups.conf"].contains("serial = \"ABC\""));
        assert!(!files["/etc/nut/ups.conf"].contains("bus ="));
        assert!(!files["/etc/nut/ups.conf"].contains("device ="));
        assert!(!files["/etc/nut/ups.conf"].contains("busport ="));
        assert!(files["/etc/nut/ups.conf"].contains("maxretry = 3"));
        assert!(files["/etc/nut/ups.conf"].contains("ignorelb"));
        assert!(files["/etc/nut/ups.conf"].contains("override.battery.charge.low = 20"));
        assert!(files["/etc/nut/ups.conf"].contains("override.battery.runtime.low = -1"));
        assert!(files["/etc/nut/ups.conf"].contains("allow_killpower"));
        assert!(!files["/etc/nut/upsmon.conf"].contains("NOTIFYCMD"));
        assert!(files["/etc/nut/upsmon.conf"].contains("HOSTSYNC 15"));
        assert!(files["/etc/nut/upsmon.conf"].contains("FINALDELAY 5"));
        assert!(files["/etc/nut/upsmon.conf"].contains("POWERDOWNFLAG /etc/killpower"));
        assert!(files["/etc/nut/upsmon.conf"].contains("SHUTDOWNCMD \"/sbin/shutdown -h +0\""));
        assert!(files["/etc/nut/upsmon.conf"].contains(" secret primary"));
        assert!(files["/etc/nut/upsd.users"].contains("password = secret"));
        assert!(files["/etc/nut/upsd.users"].contains("[nwm]"));
        assert!(files["/etc/nut/upsd.users"].contains("password = nwm"));
        assert!(files["/etc/nut/upsd.users"].contains("upsmon secondary"));

        server.shutdown.powerdown_enabled = false;
        let files = server_candidate(&server, &credentials);
        assert!(
            files["/etc/nut/upsmon.conf"].contains("POWERDOWNFLAG /run/nut/nwm-powerdown-disabled")
        );
        assert!(!files["/etc/nut/upsmon.conf"].contains("POWERDOWNFLAG /etc/killpower"));
        assert!(!files["/etc/nut/ups.conf"].contains("allow_killpower"));
        assert!(
            files["/etc/nut/upsmon.conf"].contains("SHUTDOWNCMD \"/etc/nut/nwm-shutdown-command\"")
        );
        assert!(files["/etc/nut/nwm-shutdown-command"].contains("rm -f \"$flag\""));
        assert!(files["/etc/nut/nwm-shutdown-command"].contains("upsmon -K"));
        assert_eq!(candidate_mode("/etc/nut/nwm-shutdown-command"), "750");

        server.shutdown.trigger_mode = ShutdownTriggerMode::OnBatteryTimer;
        server.shutdown.on_battery_seconds = 780;
        let files = server_candidate(&server, &credentials);
        assert!(files["/etc/nut/ups.conf"].contains("ignorelb"));
        assert!(files["/etc/nut/ups.conf"].contains("override.battery.charge.low = 20"));
        assert!(files["/etc/nut/upsmon.conf"].contains("NOTIFYCMD /usr/sbin/upssched"));
        assert!(files["/etc/nut/upssched.conf"].contains("START-TIMER nwm-on-battery-expired 780"));
        assert!(
            files["/etc/nut/upssched.conf"]
                .contains("AT ONLINE * CANCEL-TIMER nwm-on-battery-expired")
        );
        assert!(files["/etc/nut/nwm-upssched-command"].contains("upsmon -c fsd"));
        assert_eq!(candidate_mode("/etc/nut/nwm-upssched-command"), "750");
    }

    #[tokio::test]
    async fn list_var_response_is_parsed_into_dashboard_variables() {
        use tokio::{
            io::{AsyncReadExt, AsyncWriteExt},
            net::TcpListener,
        };

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 128];
            let bytes = stream.read(&mut request).await.unwrap();
            assert_eq!(&request[..bytes], b"LIST VAR ups\n");
            stream
                .write_all(
                    b"BEGIN LIST VAR ups\nVAR ups battery.charge \"100\"\nVAR ups device.model \"SANTAK TG-BOX 850\"\nVAR ups ups.status \"OL\"\nEND LIST VAR ups\n",
                )
                .await
                .unwrap();
        });

        let variables = query_ups_variables("127.0.0.1", port, "ups").await.unwrap();
        server.await.unwrap();
        assert_eq!(variables["battery.charge"], "100");
        assert_eq!(variables["device.model"], "SANTAK TG-BOX 850");
        assert_eq!(variables["ups.status"], "OL");
    }

    #[test]
    fn usb_permission_refresh_targets_the_scanned_device() {
        let server = ServerRecord {
            id: NutServerId::new(),
            host_id: HostId::new(),
            ups_name: "ups".into(),
            listen_address: "0.0.0.0".into(),
            listen_port: 3493,
            enabled: false,
            apply_state: ApplyState::Unconfigured,
            applied_revision_id: None,
            shutdown: crate::persistence::ShutdownOptions {
                trigger_mode: ShutdownTriggerMode::BatteryLevel,
                battery_level_percent: 20,
                on_battery_seconds: 300,
                host_sync_seconds: 15,
                final_delay_seconds: 5,
                powerdown_enabled: true,
            },
            device: crate::persistence::DeviceRecord {
                id: UpsId::new(),
                name: "ups".into(),
                driver: "usbhid-ups".into(),
                port: "auto".into(),
                selectors: BTreeMap::from([
                    ("vendorid".into(), "0463".into()),
                    ("productid".into(), "FFFF".into()),
                ]),
            },
        };

        let command = usb_permission_refresh(&server);
        assert!(command.contains("--attr-match=idVendor=0463"));
        assert!(command.contains("--attr-match=idProduct=ffff"));
        assert!(command.contains("udevadm settle --timeout=10"));
    }
}
