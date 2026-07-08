//! MUDL Slack bot — thin transport adapter over the shared session manager.
//!
//! Receives workspace events via HTTP Event Subscriptions and delivers responses
//! through [`GameTransport`](mudl::transport::GameTransport).

use std::sync::Arc;

use anyhow::{bail, Result};
use mudl::command::bootstrap_active_universe;
use mudl::gateway::SessionManager;
use mudl::mudl::default_module_dir;
use mudl::object::{ObjectFactory, ObjectId};
use mudl::persistence::{
    SqlitePersistence, WriterGuard, WriterLockOptions, WriterMode,
};
use mudl::slack::{
    EventsServerState, MockTransport, SlackBot, SlackConfig,
    SlackWebTransport, run_events_server,
};
use tracing::{info, warn};

fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
}

async fn open_session_manager(
    config: &SlackConfig,
    writer_guard: &WriterGuard,
) -> Result<SessionManager<SqlitePersistence>> {
    if let Some(record) = writer_guard.record() {
        info!(
            mode = record.mode.as_str(),
            pid = record.pid,
            "acquired single-writer database lock (SEC-23)"
        );
    }
    let persistence = SqlitePersistence::new(&config.database_url).await?;
    let factory = ObjectFactory::new(persistence.clone());
    let player_id = ObjectId::new(&config.default_player);

    let module_dir = default_module_dir();
    let universe = mudl::command::reload_universe(
        module_dir
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("invalid module path"))?,
    )?;
    let anatomy = universe.active_world()?.anatomy.clone();

    if let Ok((_universe, loc_id)) = bootstrap_active_universe(&factory, player_id.clone()).await {
        info!(location = %loc_id, "world bootstrapped for Slack");
    } else {
        warn!("bootstrap skipped or failed — using persisted world");
    }

    SessionManager::open_with_rate_limits(persistence, anatomy, config.rate_limits.clone())
        .await
        .map_err(Into::into)
}

#[tokio::main]
async fn main() -> Result<()> {
    mudl::env::load_project_env();
    init_tracing();

    let config = SlackConfig::from_env();
    let writer_options = WriterLockOptions::from_env(WriterMode::Slack);
    let writer_guard = WriterGuard::acquire(&config.database_url, &writer_options)
        .map_err(anyhow::Error::from)?;
    let manager = open_session_manager(&config, &writer_guard).await?;

    info!(
        connection = %config.connection_summary(),
        database = %config.database_url,
        "MUDL Slack bot starting"
    );

    if std::env::var("SLACK_MOCK").is_ok() {
        run_mock_bot(manager, config, writer_guard).await
    } else {
        run_live_bot(manager, config, writer_guard).await
    }
}

async fn run_mock_bot(
    manager: SessionManager<SqlitePersistence>,
    config: SlackConfig,
    _writer_guard: WriterGuard,
) -> Result<()> {
    let transport = Arc::new(MockTransport::new());
    let bot = SlackBot::new(manager, Arc::clone(&transport), config);
    info!("Slack mock mode — reading stdin lines as `user_id channel_id command`");

    let mut line = String::new();
    loop {
        line.clear();
        if std::io::stdin().read_line(&mut line)? == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let mut parts = trimmed.split_whitespace();
        let user_id = parts.next().unwrap_or("U_TEST");
        let channel_id = parts.next().unwrap_or("D_TEST");
        let command = parts.collect::<Vec<_>>().join(" ");
        bot.handle_input(user_id, channel_id, &command).await?;
    }
    Ok(())
}

async fn run_live_bot(
    manager: SessionManager<SqlitePersistence>,
    config: SlackConfig,
    _writer_guard: WriterGuard,
) -> Result<()> {
    if !config.has_live_credentials() {
        bail!(
            "SLACK_BOT_TOKEN and SLACK_SIGNING_SECRET are required for live mode. \
             Set SLACK_MOCK=1 for local stdin testing."
        );
    }

    let transport = Arc::new(SlackWebTransport::new(config.bot_token.clone()));
    let bot = Arc::new(SlackBot::new(manager, Arc::clone(&transport), config.clone()));
    let state = Arc::new(EventsServerState { bot, config });
    run_events_server(state).await
}