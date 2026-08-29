use std::time::Duration;

use nwm_common::Host;
use serde::Serialize;
use thiserror::Error;

use crate::ssh::{SshError, SshManager};

const DEACTIVATE_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, Serialize)]
pub struct NutDeactivateOutcome {
    pub services_stopped: bool,
    pub packages_removed: bool,
    pub remote_configuration_removed: bool,
    pub public_key_removed: bool,
}

#[derive(Debug, Error)]
pub enum NutDeactivateError {
    #[error(transparent)]
    Ssh(#[from] SshError),
}

pub async fn deactivate(
    ssh: &SshManager,
    host: &Host,
) -> Result<NutDeactivateOutcome, NutDeactivateError> {
    ssh.execute_script(host, deactivate_script(), DEACTIVATE_TIMEOUT)
        .await?;
    Ok(NutDeactivateOutcome {
        services_stopped: true,
        packages_removed: false,
        remote_configuration_removed: false,
        public_key_removed: false,
    })
}

fn deactivate_script() -> &'static str {
    r#"set -eu
systemctl disable --now nut-monitor.service nut-server.service nut-driver-enumerator.path nut-driver.target >/dev/null 2>&1 || true
for unit in $(systemctl list-units --all --plain --no-legend 'nut-driver@*.service' 2>/dev/null | awk '{print $1}'); do
    systemctl stop "$unit" >/dev/null 2>&1 || true
done
pkill -TERM -x upssched >/dev/null 2>&1 || true
rm -f /run/nut/nwm-upssched.pipe /run/nut/nwm-upssched.lock /run/nut/nwm-powerdown-disabled
for unit in nut-monitor.service nut-server.service nut-driver.target; do
    if systemctl is-active --quiet "$unit"; then
        echo "NUT service is still active: $unit" >&2
        exit 1
    fi
done
"#
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deactivation_only_stops_services_and_runtime_processes() {
        let script = deactivate_script();
        assert!(script.contains("systemctl disable --now"));
        assert!(!script.contains("apt-get"));
        assert!(!script.contains("apt remove"));
        assert!(!script.contains("/etc/nut"));
        assert!(!script.contains("authorized_keys"));
    }
}
