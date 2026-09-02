pub mod protocol;

use std::time::Duration;

use chrono::Utc;
use tokio::{task::JoinSet, time};

use crate::{
    persistence::{Database, MonitorSource},
    ups_monitor::protocol::{list_ups, list_variables},
};

const POLL_INTERVAL: Duration = Duration::from_secs(5);

pub async fn run_collector(database: Database) {
    let mut interval = time::interval(POLL_INTERVAL);
    interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
    let mut last_prune = Utc::now() - chrono::Duration::days(1);
    loop {
        interval.tick().await;
        if let Err(error) = collect_once(&database).await {
            tracing::error!(%error, "UPS monitor collection cycle failed");
        }
        if Utc::now() - last_prune >= chrono::Duration::days(1) {
            match database.monitor().prune_history().await {
                Ok(removed) if removed > 0 => {
                    tracing::info!(removed, "pruned expired UPS monitor history")
                }
                Ok(_) => {}
                Err(error) => tracing::warn!(%error, "failed to prune UPS monitor history"),
            }
            last_prune = Utc::now();
        }
    }
}

async fn collect_once(database: &Database) -> Result<(), crate::persistence::PersistenceError> {
    let sources = database.monitor().enabled_sources().await?;
    let mut tasks = JoinSet::new();
    for source in sources {
        let database = database.clone();
        tasks.spawn(async move { collect_source(database, source).await });
    }
    while let Some(result) = tasks.join_next().await {
        match result {
            Ok(Ok(())) => {}
            Ok(Err(error)) => tracing::warn!(%error, "UPS monitor source collection failed"),
            Err(error) => tracing::warn!(%error, "UPS monitor collection task failed"),
        }
    }
    Ok(())
}

async fn collect_source(
    database: Database,
    source: MonitorSource,
) -> Result<(), crate::persistence::PersistenceError> {
    let repository = database.monitor();
    let mut devices = repository.devices_for_source(&source.id).await?;
    if devices.is_empty() || repository.discovery_due(&source).await {
        match list_ups(&source.address, source.port).await {
            Ok(discovered) => devices = repository.sync_discovery(&source.id, &discovered).await?,
            Err(error) => {
                tracing::debug!(source = %source.name, %error, "UPS discovery failed");
                repository.source_failure(&source.id, &error).await?;
                return Ok(());
            }
        }
    }
    for device in devices {
        match list_variables(&source.address, source.port, &device.ups_name).await {
            Ok(raw) => {
                repository
                    .record_success(&device.id, &raw, Utc::now())
                    .await?
            }
            Err(error) => {
                repository
                    .record_failure(&device.id, &error, Utc::now())
                    .await?
            }
        }
    }
    Ok(())
}
