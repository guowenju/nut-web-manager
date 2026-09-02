use std::{collections::HashMap, time::Duration};

use chrono::Utc;
use tokio::time;

use crate::{
    nut,
    persistence::{Database, PersistenceError},
};

pub async fn run_collector(database: Database) {
    let mut interval = time::interval(Duration::from_secs(60));
    interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
    let mut cycles = 0_u16;
    loop {
        interval.tick().await;
        if let Err(error) = collect_once(&database).await {
            tracing::debug!(%error, "managed UPS history collection failed");
        }
        cycles += 1;
        if cycles >= 1_440 {
            if let Err(error) = database.dashboard_history().prune().await {
                tracing::warn!(%error, "failed to prune managed UPS history");
            }
            cycles = 0;
        }
    }
}

async fn collect_once(database: &Database) -> Result<(), PersistenceError> {
    let server = database
        .topology()
        .list_servers()
        .await?
        .into_iter()
        .find(|server| server.enabled);
    let Some(server) = server else {
        return Ok(());
    };
    let Some(host) = database.hosts().get(server.host_id).await? else {
        return Ok(());
    };
    let raw =
        match nut::query_ups_variables(&host.address, server.listen_port, &server.ups_name).await {
            Ok(raw) => raw,
            Err(error) => {
                tracing::debug!(server = %server.id, %error, "managed UPS history query failed");
                return Ok(());
            }
        };
    let reported_runtime: Option<i64> = number(&raw, "battery.runtime");
    database
        .dashboard_history()
        .record(
            &server.id.to_string(),
            Utc::now(),
            number(&raw, "ups.load"),
            reported_runtime.filter(|value| *value != 65_535),
            number(&raw, "ups.realpower"),
        )
        .await
}

fn number<T: std::str::FromStr>(raw: &HashMap<String, String>, key: &str) -> Option<T> {
    raw.get(key)?.parse().ok()
}
