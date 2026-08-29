use anyhow::Context;
use nwm::{app, config::Settings, persistence::Database, ssh::SshManager, state::AppState};
use tokio::signal;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    load_dotenv()?;
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "nwm=info,tower_http=info".into()),
        )
        .init();

    let settings = Settings::from_env().context("failed to load application settings")?;
    settings.prepare_data_dir().with_context(|| {
        format!(
            "failed to prepare the data directory {}",
            settings.data_dir.display()
        )
    })?;
    let ssh = SshManager::initialize(&settings.data_dir)
        .await
        .context("failed to initialize managed SSH identity")?;

    let database = Database::connect(&settings.database_url)
        .await
        .context("failed to connect to SQLite")?;
    database
        .migrate()
        .await
        .context("failed to migrate SQLite")?;
    let interrupted = database
        .operations()
        .fail_incomplete_after_restart()
        .await
        .context("failed to recover interrupted operations")?;
    if interrupted > 0 {
        tracing::warn!(
            count = interrupted,
            "marked interrupted operations as failed"
        );
    }

    let listener = tokio::net::TcpListener::bind(settings.bind_address)
        .await
        .with_context(|| format!("failed to bind {}", settings.bind_address))?;

    let state = AppState::new(database.clone(), settings, ssh);
    info!(address = %state.settings.bind_address, "NUT Web Manager listening");

    let server_result = axum::serve(listener, app::router(state))
        .with_graceful_shutdown(shutdown_signal())
        .await;

    info!("closing database connection pool");
    database.close().await;
    info!("database connection pool closed");

    server_result.context("HTTP server failed")
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    info!("shutdown signal received; draining HTTP connections");
}

fn load_dotenv() -> anyhow::Result<()> {
    match dotenvy::dotenv() {
        Ok(_) => Ok(()),
        Err(dotenvy::Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).context("failed to load .env"),
    }
}
