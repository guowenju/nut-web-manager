use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};

use nwm_common::{Host, PlatformInfo, PlatformKind};
use serde::Serialize;
use thiserror::Error;
use tokio::{io::AsyncWriteExt, process::Command, time::timeout};

const COMMAND_TIMEOUT: Duration = Duration::from_secs(12);
const KEYSCAN_TIMEOUT: Duration = Duration::from_secs(8);
const ENVIRONMENT_SCRIPT: &str = r#"
set -eu
. /etc/os-release
printf 'OS_ID=%s\n' "${ID:-}"
printf 'OS_VERSION=%s\n' "${VERSION_ID:-}"
printf 'HOSTNAME=%s\n' "$(hostname)"
if command -v pveversion >/dev/null 2>&1; then
  printf 'PVE_VERSION=%s\n' "$(pveversion | head -n 1)"
fi
if command -v proxmox-backup-manager >/dev/null 2>&1; then
  printf 'PBS_VERSION=%s\n' "$(proxmox-backup-manager version 2>/dev/null | head -n 1)"
fi
if command -v systemctl >/dev/null 2>&1; then
  printf 'SYSTEMD_VERSION=%s\n' "$(systemctl --version | head -n 1)"
fi
package_version() {
  package_status="$(dpkg-query -W -f='${db:Status-Abbrev}|${Version}' "$1" 2>/dev/null || true)"
  case "$package_status" in
    "ii |"*) printf '%s' "${package_status#*|}" ;;
  esac
}
printf 'NUT_SERVER=%s\n' "$(package_version nut-server)"
printf 'NUT_CLIENT=%s\n' "$(package_version nut-client)"
"#;

#[derive(Clone)]
pub struct SshManager {
    private_key: PathBuf,
    public_key: Arc<str>,
    known_hosts: PathBuf,
    known_hosts_write: Arc<Mutex<()>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HostKeyState {
    Trusted,
    ConfirmationRequired,
    Changed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct HostKeyInspection {
    pub state: HostKeyState,
    pub algorithm: String,
    pub fingerprint: String,
    #[serde(skip)]
    known_hosts_line: String,
    #[serde(skip)]
    host_token: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SshTestReport {
    pub connected: bool,
    pub host_key: HostKeyInspection,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct EnvironmentReport {
    pub platform: PlatformInfo,
    pub supported: bool,
    pub systemd_version: Option<String>,
    pub nut_server_installed: bool,
    pub nut_client_installed: bool,
    pub nut_server_version: Option<String>,
    pub nut_client_version: Option<String>,
}

#[derive(Debug, Error)]
pub enum SshError {
    #[error("SSH filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("{program} timed out")]
    Timeout { program: &'static str },
    #[error("{program} failed: {detail}")]
    Command {
        program: &'static str,
        detail: String,
    },
    #[error("SSH output was invalid: {0}")]
    InvalidOutput(String),
    #[error("host key fingerprint no longer matches the scanned host")]
    FingerprintMismatch,
    #[error("host key confirmation is required")]
    HostKeyConfirmationRequired,
    #[error("the trusted host key has changed")]
    HostKeyChanged,
}

impl SshManager {
    pub async fn initialize(data_dir: &Path) -> Result<Self, SshError> {
        let ssh_dir = data_dir.join("ssh");
        fs::create_dir_all(&ssh_dir)?;
        set_mode(&ssh_dir, 0o700)?;

        let private_key = ssh_dir.join("id_ed25519");
        let public_key_path = ssh_dir.join("id_ed25519.pub");
        if !private_key.exists() {
            let output = run_command(
                "ssh-keygen",
                [
                    "-q",
                    "-t",
                    "ed25519",
                    "-N",
                    "",
                    "-C",
                    "nut-web-manager",
                    "-f",
                    path_text(&private_key)?,
                ],
                COMMAND_TIMEOUT,
            )
            .await?;
            ensure_success("ssh-keygen", output)?;
        }
        set_mode(&private_key, 0o600)?;

        let output = run_command(
            "ssh-keygen",
            ["-y", "-f", path_text(&private_key)?],
            COMMAND_TIMEOUT,
        )
        .await?;
        let derived_public_key =
            String::from_utf8_lossy(&ensure_success("ssh-keygen", output)?.stdout)
                .trim()
                .to_owned();
        if !public_key_path.exists() {
            let public_key = format!("{derived_public_key} nut-web-manager\n");
            write_private_file(&public_key_path, public_key.as_bytes(), 0o644)?;
        }

        let public_key = fs::read_to_string(&public_key_path)?.trim().to_owned();
        if !public_key.starts_with("ssh-ed25519 ") {
            return Err(SshError::InvalidOutput(
                "managed public key is not Ed25519".into(),
            ));
        }
        if key_material(&public_key) != key_material(&derived_public_key) {
            return Err(SshError::InvalidOutput(
                "managed public key does not match the private key".into(),
            ));
        }

        let known_hosts = ssh_dir.join("known_hosts");
        if !known_hosts.exists() {
            write_private_file(&known_hosts, &[], 0o600)?;
        }
        set_mode(&known_hosts, 0o600)?;

        Ok(Self {
            private_key,
            public_key: public_key.into(),
            known_hosts,
            known_hosts_write: Arc::new(Mutex::new(())),
        })
    }

    pub fn public_key(&self) -> &str {
        &self.public_key
    }

    pub async fn inspect_host_key(&self, host: &Host) -> Result<HostKeyInspection, SshError> {
        let output = run_command(
            "ssh-keyscan",
            [
                "-T".to_owned(),
                "5".to_owned(),
                "-p".to_owned(),
                host.ssh_port.to_string(),
                "-t".to_owned(),
                "ed25519".to_owned(),
                host.address.clone(),
            ],
            KEYSCAN_TIMEOUT,
        )
        .await?;
        let output = ensure_success("ssh-keyscan", output)?;
        let line = String::from_utf8_lossy(&output.stdout)
            .lines()
            .find(|line| !line.trim().is_empty() && !line.starts_with('#'))
            .ok_or_else(|| SshError::InvalidOutput("ssh-keyscan returned no Ed25519 key".into()))?
            .trim()
            .to_owned();
        let scanned = ParsedHostKey::parse(&line)?;
        let fingerprint = fingerprint(&line).await?;

        let known_hosts = fs::read_to_string(&self.known_hosts)?;
        let matching_host: Vec<_> = known_hosts
            .lines()
            .filter_map(|line| ParsedHostKey::parse(line).ok())
            .filter(|key| key.contains_host(&scanned.hosts))
            .collect();
        let state = if matching_host
            .iter()
            .any(|key| key.algorithm == scanned.algorithm && key.key == scanned.key)
        {
            HostKeyState::Trusted
        } else if matching_host.is_empty() {
            HostKeyState::ConfirmationRequired
        } else {
            HostKeyState::Changed
        };

        Ok(HostKeyInspection {
            state,
            algorithm: scanned.algorithm,
            fingerprint,
            known_hosts_line: line,
            host_token: scanned.hosts,
        })
    }

    pub async fn trust_host_key(
        &self,
        host: &Host,
        expected_fingerprint: &str,
    ) -> Result<HostKeyInspection, SshError> {
        let inspection = self.inspect_host_key(host).await?;
        if inspection.fingerprint != expected_fingerprint {
            return Err(SshError::FingerprintMismatch);
        }

        let _guard = self
            .known_hosts_write
            .lock()
            .expect("known_hosts lock poisoned");
        let existing = fs::read_to_string(&self.known_hosts)?;
        let mut retained: Vec<&str> = existing
            .lines()
            .filter(|line| {
                ParsedHostKey::parse(line)
                    .map(|key| !key.contains_host(&inspection.host_token))
                    .unwrap_or(true)
            })
            .collect();
        retained.push(&inspection.known_hosts_line);
        let mut contents = retained.join("\n");
        contents.push('\n');

        let temporary = self.known_hosts.with_extension("tmp");
        write_private_file(&temporary, contents.as_bytes(), 0o600)?;
        fs::rename(temporary, &self.known_hosts)?;

        Ok(HostKeyInspection {
            state: HostKeyState::Trusted,
            ..inspection
        })
    }

    pub async fn test(&self, host: &Host) -> Result<SshTestReport, SshError> {
        let host_key = self.inspect_host_key(host).await?;
        if host_key.state != HostKeyState::Trusted {
            return Ok(SshTestReport {
                connected: false,
                host_key,
            });
        }

        let output = self.run_ssh(host, ["true"], None, COMMAND_TIMEOUT).await?;
        ensure_success("ssh", output)?;
        Ok(SshTestReport {
            connected: true,
            host_key,
        })
    }

    pub async fn environment(&self, host: &Host) -> Result<EnvironmentReport, SshError> {
        match self.inspect_host_key(host).await?.state {
            HostKeyState::Trusted => {}
            HostKeyState::ConfirmationRequired => {
                return Err(SshError::HostKeyConfirmationRequired);
            }
            HostKeyState::Changed => return Err(SshError::HostKeyChanged),
        }

        let output = self
            .run_ssh(
                host,
                ["sh", "-s"],
                Some(ENVIRONMENT_SCRIPT.as_bytes()),
                COMMAND_TIMEOUT,
            )
            .await?;
        let output = ensure_success("ssh", output)?;
        parse_environment(&String::from_utf8_lossy(&output.stdout))
    }

    pub async fn execute_script(
        &self,
        host: &Host,
        script: &str,
        duration: Duration,
    ) -> Result<String, SshError> {
        match self.inspect_host_key(host).await?.state {
            HostKeyState::Trusted => {}
            HostKeyState::ConfirmationRequired => {
                return Err(SshError::HostKeyConfirmationRequired);
            }
            HostKeyState::Changed => return Err(SshError::HostKeyChanged),
        }

        let output = self
            .run_ssh(host, ["sh", "-s"], Some(script.as_bytes()), duration)
            .await?;
        let output = ensure_success("ssh", output)?;
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    async fn run_ssh<I, S>(
        &self,
        host: &Host,
        remote_command: I,
        stdin: Option<&[u8]>,
        duration: Duration,
    ) -> Result<std::process::Output, SshError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        let mut command = Command::new("ssh");
        command
            .args([
                "-i",
                path_text(&self.private_key)?,
                "-o",
                "BatchMode=yes",
                "-o",
                "IdentitiesOnly=yes",
                "-o",
                "PasswordAuthentication=no",
                "-o",
                "KbdInteractiveAuthentication=no",
                "-o",
                "StrictHostKeyChecking=yes",
                "-o",
                &format!("UserKnownHostsFile={}", self.known_hosts.display()),
                "-o",
                "ConnectTimeout=8",
                "-p",
                &host.ssh_port.to_string(),
            ])
            .arg(format!("{}@{}", host.username, host.address))
            .args(remote_command)
            .kill_on_drop(true)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if stdin.is_some() {
            command.stdin(Stdio::piped());
        } else {
            command.stdin(Stdio::null());
        }

        let mut child = command.spawn()?;
        if let Some(input) = stdin {
            child
                .stdin
                .take()
                .expect("piped SSH stdin must be available")
                .write_all(input)
                .await?;
        }
        timeout(duration, child.wait_with_output())
            .await
            .map_err(|_| SshError::Timeout { program: "ssh" })?
            .map_err(Into::into)
    }
}

struct ParsedHostKey {
    hosts: String,
    algorithm: String,
    key: String,
}

impl ParsedHostKey {
    fn parse(line: &str) -> Result<Self, SshError> {
        let mut fields = line.split_whitespace();
        let hosts = fields
            .next()
            .ok_or_else(|| SshError::InvalidOutput("known_hosts entry has no host".into()))?;
        if hosts.starts_with('#') || hosts.starts_with('|') {
            return Err(SshError::InvalidOutput(
                "comments and hashed known_hosts entries are not host keys".into(),
            ));
        }
        let algorithm = fields
            .next()
            .ok_or_else(|| SshError::InvalidOutput("known_hosts entry has no algorithm".into()))?;
        let key = fields
            .next()
            .ok_or_else(|| SshError::InvalidOutput("known_hosts entry has no key".into()))?;
        Ok(Self {
            hosts: hosts.to_owned(),
            algorithm: algorithm.to_owned(),
            key: key.to_owned(),
        })
    }

    fn contains_host(&self, target: &str) -> bool {
        self.hosts
            .split(',')
            .any(|candidate| target.split(',').any(|target| candidate == target))
    }
}

async fn fingerprint(known_hosts_line: &str) -> Result<String, SshError> {
    let output = run_command_with_stdin(
        "ssh-keygen",
        ["-lf", "-", "-E", "sha256"],
        known_hosts_line.as_bytes(),
        COMMAND_TIMEOUT,
    )
    .await?;
    let output = ensure_success("ssh-keygen", output)?;
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .nth(1)
        .map(str::to_owned)
        .ok_or_else(|| SshError::InvalidOutput("ssh-keygen returned no fingerprint".into()))
}

fn parse_environment(output: &str) -> Result<EnvironmentReport, SshError> {
    let values: HashMap<&str, &str> = output
        .lines()
        .filter_map(|line| line.split_once('='))
        .collect();
    let os_id = values.get("OS_ID").copied().unwrap_or_default();
    let os_version = values.get("OS_VERSION").copied().unwrap_or_default();
    let hostname = values.get("HOSTNAME").copied().unwrap_or_default();
    if hostname.is_empty() {
        return Err(SshError::InvalidOutput(
            "environment probe returned no hostname".into(),
        ));
    }

    let pve_version = non_empty(values.get("PVE_VERSION").copied());
    let pbs_version = non_empty(values.get("PBS_VERSION").copied());
    let kind = if pve_version.is_some() {
        PlatformKind::ProxmoxVe
    } else if pbs_version.is_some() {
        PlatformKind::ProxmoxBackupServer
    } else if os_id == "debian" {
        PlatformKind::Debian
    } else {
        PlatformKind::Unsupported
    };
    let product_version = pve_version.or(pbs_version);
    let systemd_version = non_empty(values.get("SYSTEMD_VERSION").copied());
    let supported = os_id == "debian"
        && os_version.split('.').next() == Some("13")
        && systemd_version.is_some()
        && match kind {
            PlatformKind::ProxmoxVe => product_version
                .as_deref()
                .is_some_and(|version| version.contains("/9.")),
            PlatformKind::ProxmoxBackupServer => product_version
                .as_deref()
                .is_some_and(|version| version.contains(" 4.")),
            PlatformKind::Debian => true,
            PlatformKind::Unsupported => false,
        };
    let nut_server = non_empty(values.get("NUT_SERVER").copied());
    let nut_client = non_empty(values.get("NUT_CLIENT").copied());

    Ok(EnvironmentReport {
        platform: PlatformInfo {
            kind: if supported {
                kind
            } else {
                PlatformKind::Unsupported
            },
            os_version: os_version.to_owned(),
            product_version,
            hostname: hostname.to_owned(),
            nut_version: nut_server.clone().or_else(|| nut_client.clone()),
        },
        supported,
        systemd_version,
        nut_server_installed: nut_server.is_some(),
        nut_client_installed: nut_client.is_some(),
        nut_server_version: nut_server,
        nut_client_version: nut_client,
    })
}

fn non_empty(value: Option<&str>) -> Option<String> {
    value
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
}

fn key_material(value: &str) -> Option<(&str, &str)> {
    let mut fields = value.split_whitespace();
    Some((fields.next()?, fields.next()?))
}

async fn run_command<I, S>(
    program: &'static str,
    args: I,
    duration: Duration,
) -> Result<std::process::Output, SshError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let mut command = Command::new(program);
    command.args(args).kill_on_drop(true);
    timeout(duration, command.output())
        .await
        .map_err(|_| SshError::Timeout { program })?
        .map_err(Into::into)
}

async fn run_command_with_stdin<I, S>(
    program: &'static str,
    args: I,
    input: &[u8],
    duration: Duration,
) -> Result<std::process::Output, SshError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    child
        .stdin
        .take()
        .expect("piped command stdin must be available")
        .write_all(input)
        .await?;
    timeout(duration, child.wait_with_output())
        .await
        .map_err(|_| SshError::Timeout { program })?
        .map_err(Into::into)
}

fn ensure_success(
    program: &'static str,
    output: std::process::Output,
) -> Result<std::process::Output, SshError> {
    if output.status.success() {
        return Ok(output);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}\n{}", stdout.trim(), stderr.trim());
    let detail: String = combined.trim().chars().take(4000).collect();
    Err(SshError::Command {
        program,
        detail: if detail.is_empty() {
            format!("exited with {}", output.status)
        } else {
            detail
        },
    })
}

fn path_text(path: &Path) -> Result<&str, SshError> {
    path.to_str()
        .ok_or_else(|| SshError::InvalidOutput("SSH path is not valid UTF-8".into()))
}

fn write_private_file(path: &Path, contents: &[u8], mode: u32) -> Result<(), SshError> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode);
    }
    std::io::Write::write_all(&mut options.open(path)?, contents)?;
    set_mode(path, mode)
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> Result<(), SshError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> Result<(), SshError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsed_host_key_matches_comma_separated_hosts() {
        let key = ParsedHostKey::parse("host,[host]:2222 ssh-ed25519 AAAA comment").unwrap();
        assert!(key.contains_host("[host]:2222"));
        assert!(!key.contains_host("other"));
    }

    #[test]
    fn environment_probe_accepts_supported_pve() {
        let report = parse_environment(
            "OS_ID=debian\nOS_VERSION=13\nHOSTNAME=pve\nPVE_VERSION=pve-manager/9.0.1/abc\nSYSTEMD_VERSION=systemd 257\nNUT_SERVER=2.8.1\nNUT_CLIENT=\n",
        )
        .unwrap();
        assert!(report.supported);
        assert_eq!(report.platform.kind, PlatformKind::ProxmoxVe);
        assert!(report.nut_server_installed);
    }

    #[test]
    fn environment_probe_marks_old_debian_unsupported() {
        let report = parse_environment(
            "OS_ID=debian\nOS_VERSION=12\nHOSTNAME=legacy\nNUT_SERVER=\nNUT_CLIENT=\n",
        )
        .unwrap();
        assert!(!report.supported);
        assert_eq!(report.platform.kind, PlatformKind::Unsupported);
    }
}
