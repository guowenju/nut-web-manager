use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{ApplyState, HostId, ProtectionHealth};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TopologyNodeKind {
    Ups,
    Server,
    Client,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TopologyEdgeKind {
    UsbAttached,
    NutMonitors,
    PoweredBy,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TopologyNode {
    pub id: String,
    pub kind: TopologyNodeKind,
    pub label: String,
    pub host_id: Option<HostId>,
    pub health: ProtectionHealth,
    pub last_verified_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TopologyEdge {
    pub id: String,
    pub kind: TopologyEdgeKind,
    pub source: String,
    pub target: String,
    pub apply_state: ApplyState,
    pub health: ProtectionHealth,
    pub last_verified_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TopologySnapshot {
    pub nodes: Vec<TopologyNode>,
    pub edges: Vec<TopologyEdge>,
    pub observed_at: DateTime<Utc>,
}
