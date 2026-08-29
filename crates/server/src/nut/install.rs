use std::time::Duration;

use nwm_common::{Host, HostRole, PlatformInfo};
use serde::Serialize;
use thiserror::Error;

use crate::ssh::{EnvironmentReport, SshError, SshManager};

const INSTALL_TIMEOUT: Duration = Duration::from_secs(10 * 60);

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct NutInstallStatus {
    pub role: HostRole,
    pub platform: PlatformInfo,
    pub package: &'static str,
    pub installed: bool,
    pub version: Option<String>,
    pub install_command: String,
    pub automatic_install_available: bool,
    pub already_installed: bool,
}

#[derive(Debug, Error)]
pub enum NutInstallError {
    #[error(transparent)]
    Ssh(#[from] SshError),
    #[error("the detected platform is not supported for automatic installation")]
    UnsupportedPlatform,
    #[error("another modifying operation is already running on this host")]
    HostOperationInProgress,
    #[error("automatic installation was not explicitly confirmed")]
    ConfirmationRequired,
    #[error("automatic installation timed out")]
    InstallTimedOut,
    #[error("{code}: {detail}")]
    InstallFailed {
        code: InstallFailureCode,
        detail: String,
    },
    #[error("APT completed but {package} is still not installed")]
    VerificationFailed { package: &'static str },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstallFailureCode {
    AptRepositoryError,
    DpkgLock,
    NetworkFailure,
    PermissionDenied,
    UnsupportedPackageState,
}

impl InstallFailureCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AptRepositoryError => "AptRepositoryError",
            Self::DpkgLock => "DpkgLock",
            Self::NetworkFailure => "NetworkFailure",
            Self::PermissionDenied => "PermissionDenied",
            Self::UnsupportedPackageState => "UnsupportedPackageState",
        }
    }
}

impl std::fmt::Display for InstallFailureCode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

pub async fn status(ssh: &SshManager, host: &Host) -> Result<NutInstallStatus, NutInstallError> {
    let environment = ssh.environment(host).await?;
    status_from_environment(host.role, &environment)
}

pub async fn install(ssh: &SshManager, host: &Host) -> Result<NutInstallStatus, NutInstallError> {
    let before = ssh.environment(host).await?;
    let before_status = status_from_environment(host.role, &before)?;
    if before_status.installed {
        return Ok(before_status);
    }

    let script = install_script(before_status.package);
    match ssh.execute_script(host, &script, INSTALL_TIMEOUT).await {
        Ok(_) => {}
        Err(SshError::Timeout { .. }) => return Err(NutInstallError::InstallTimedOut),
        Err(SshError::Command { detail, .. }) => {
            return Err(NutInstallError::InstallFailed {
                code: classify_install_failure(&detail),
                detail,
            });
        }
        Err(error) => return Err(error.into()),
    }

    let after = ssh.environment(host).await?;
    let status = status_from_environment(host.role, &after)?;
    if !status.installed {
        return Err(NutInstallError::VerificationFailed {
            package: status.package,
        });
    }
    Ok(status)
}

fn status_from_environment(
    role: HostRole,
    environment: &EnvironmentReport,
) -> Result<NutInstallStatus, NutInstallError> {
    if !environment.supported {
        return Err(NutInstallError::UnsupportedPlatform);
    }
    let package = "nut";
    let version = environment
        .nut_server_version
        .clone()
        .or_else(|| environment.nut_client_version.clone());
    let installed = environment.nut_server_installed && environment.nut_client_installed;
    Ok(NutInstallStatus {
        role,
        platform: environment.platform.clone(),
        package,
        installed,
        version,
        install_command: "apt-get update && apt-get install -y nut".into(),
        automatic_install_available: !installed,
        already_installed: installed,
    })
}

fn install_script(package: &str) -> String {
    format!(
        "set -eu\nexport DEBIAN_FRONTEND=noninteractive\napt-get update\napt-get install -y {package}\n"
    )
}

fn classify_install_failure(detail: &str) -> InstallFailureCode {
    let lower = detail.to_ascii_lowercase();
    if lower.contains("could not get lock")
        || lower.contains("unable to acquire the dpkg frontend lock")
        || lower.contains("is another process using it")
    {
        InstallFailureCode::DpkgLock
    } else if lower.contains("temporary failure resolving")
        || lower.contains("network is unreachable")
        || lower.contains("connection failed")
        || lower.contains("could not connect")
    {
        InstallFailureCode::NetworkFailure
    } else if lower.contains("does not have a release file")
        || lower.contains("the repository") && lower.contains("is not signed")
        || lower.contains("failed to fetch")
        || lower.contains("some index files failed")
    {
        InstallFailureCode::AptRepositoryError
    } else if lower.contains("permission denied")
        || lower.contains("are you root")
        || lower.contains("operation not permitted")
    {
        InstallFailureCode::PermissionDenied
    } else {
        InstallFailureCode::UnsupportedPackageState
    }
}

#[cfg(test)]
mod tests {
    use nwm_common::{PlatformInfo, PlatformKind};

    use super::*;

    fn environment() -> EnvironmentReport {
        EnvironmentReport {
            platform: PlatformInfo {
                kind: PlatformKind::ProxmoxVe,
                os_version: "13".into(),
                product_version: Some("pve-manager/9.0.1".into()),
                hostname: "pve".into(),
                nut_version: None,
            },
            supported: true,
            systemd_version: Some("systemd 257".into()),
            nut_server_installed: false,
            nut_client_installed: true,
            nut_server_version: None,
            nut_client_version: Some("2.8.1-5".into()),
        }
    }

    #[test]
    fn every_role_installs_the_nut_meta_package() {
        let server = status_from_environment(HostRole::Server, &environment()).unwrap();
        assert_eq!(server.package, "nut");
        assert!(!server.installed);
        assert_eq!(
            server.install_command,
            "apt-get update && apt-get install -y nut"
        );

        let client = status_from_environment(HostRole::Client, &environment()).unwrap();
        assert_eq!(client.package, "nut");
        assert!(!client.installed);
    }

    #[test]
    fn apt_failures_have_actionable_codes() {
        assert_eq!(
            classify_install_failure("Could not get lock /var/lib/dpkg/lock-frontend"),
            InstallFailureCode::DpkgLock
        );
        assert_eq!(
            classify_install_failure("Temporary failure resolving deb.debian.org"),
            InstallFailureCode::NetworkFailure
        );
        assert_eq!(
            classify_install_failure("The repository x does not have a Release file"),
            InstallFailureCode::AptRepositoryError
        );
    }
}
