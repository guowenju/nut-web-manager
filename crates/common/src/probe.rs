use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{BindingId, HostId, ObservationError, UpsId};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "id", rename_all = "snake_case")]
pub enum ProbeTarget {
    Host(HostId),
    Ups(UpsId),
    Binding(BindingId),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeOutcome {
    Passed,
    Degraded,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeCheckStatus {
    Ok,
    Missing,
    Stale,
    Unreachable,
    Invalid,
    Skipped,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProbeCheck {
    pub key: String,
    pub required: bool,
    pub status: ProbeCheckStatus,
    pub observed_value: Option<String>,
    pub error: Option<ObservationError>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProbeReport {
    pub target: ProbeTarget,
    pub outcome: ProbeOutcome,
    pub summary: String,
    pub checks: Vec<ProbeCheck>,
    pub observed_at: DateTime<Utc>,
}

impl ProbeReport {
    pub fn from_checks(
        target: ProbeTarget,
        summary: impl Into<String>,
        checks: Vec<ProbeCheck>,
    ) -> Self {
        let outcome = if checks
            .iter()
            .any(|check| check.required && check.status != ProbeCheckStatus::Ok)
        {
            ProbeOutcome::Failed
        } else if checks
            .iter()
            .any(|check| check.status != ProbeCheckStatus::Ok)
        {
            ProbeOutcome::Degraded
        } else {
            ProbeOutcome::Passed
        };

        Self {
            target,
            outcome,
            summary: summary.into(),
            checks,
            observed_at: Utc::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(required: bool, status: ProbeCheckStatus) -> ProbeCheck {
        ProbeCheck {
            key: "ups.status".into(),
            required,
            status,
            observed_value: None,
            error: None,
        }
    }

    #[test]
    fn required_failure_fails_the_report() {
        let report = ProbeReport::from_checks(
            ProbeTarget::Ups(UpsId::new()),
            "probe",
            vec![check(true, ProbeCheckStatus::Missing)],
        );
        assert_eq!(report.outcome, ProbeOutcome::Failed);
    }

    #[test]
    fn optional_failure_degrades_the_report() {
        let report = ProbeReport::from_checks(
            ProbeTarget::Ups(UpsId::new()),
            "probe",
            vec![check(false, ProbeCheckStatus::Missing)],
        );
        assert_eq!(report.outcome, ProbeOutcome::Degraded);
    }
}
