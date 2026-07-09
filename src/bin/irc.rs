//! MUDL IRC bot — thin transport adapter over the shared session manager.
//!
//! Connects to IRCv3 servers over TLS by default (see `IRC_TLS`, `IRC_PORT`).

use std::sync::Arc;

use anyhow::Result;
use mudl::command::bootstrap_active_universe;
use mudl::gateway::{ensure_bootstrap_wizard, SessionManager};
use mudl::irc::{
    connect, run_irc_session, ExponentialBackoff, IrcBot, IrcConfig, MockTransport, SessionEnd,
    SessionFatal,
};
use mudl::mudl::default_module_dir;
use mudl::object::{ObjectFactory, ObjectId};
use mudl::persistence::{
    SqlitePersistence, WriterGuard, WriterLockOptions, WriterMode,
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
    config: &IrcConfig,
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

    let bootstrap_location =
        if let Ok((_universe, loc_id)) = bootstrap_active_universe(&factory, player_id.clone()).await
        {
            info!(location = %loc_id, "world bootstrapped for IRC");
            Some(loc_id)
        } else {
            warn!("bootstrap skipped or failed — using persisted world");
            None
        };

    match ensure_bootstrap_wizard(&factory, &player_id, &anatomy, bootstrap_location).await {
        Ok(true) => info!(
            player = %player_id,
            "created bootstrap wizard player from DEFAULT_PLAYER"
        ),
        Ok(false) => {}
        Err(err) => warn!(error = %err, "bootstrap wizard setup failed"),
    }

    SessionManager::open_with_rate_limits(persistence, anatomy, config.rate_limits.clone())
        .await
        .map_err(Into::into)
}

#[tokio::main]
async fn main() -> Result<()> {
    mudl::env::load_project_env();
    init_tracing();

    let config = IrcConfig::from_env();
    let writer_options = WriterLockOptions::from_env(WriterMode::Irc);
    let writer_guard = WriterGuard::acquire(&config.database_url, &writer_options)
        .map_err(anyhow::Error::from)?;
    let manager = open_session_manager(&config, &writer_guard).await?;
    info!(
        connection = %config.connection_summary(),
        nick = %config.bot_nick,
        world_channel = %config.world_channel,
        database = %config.database_url,
        reconnect = config.reconnect.enabled,
        "MUDL IRC bot starting"
    );

    if std::env::var("IRC_MOCK").is_ok() {
        run_mock_bot(manager, config, writer_guard).await
    } else {
        run_live_bot(manager, config, writer_guard).await
    }
}

async fn run_mock_bot(
    manager: SessionManager<SqlitePersistence>,
    config: IrcConfig,
    _writer_guard: WriterGuard,
) -> Result<()> {
    let transport = Arc::new(MockTransport::new());
    let bot = IrcBot::new(manager, Arc::clone(&transport), config);
    info!("IRC mock mode — reading stdin lines as PRIVMSG to bot (no TLS)");

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
        let (nick, text) = trimmed
            .split_once(' ')
            .map(|(n, t)| (n, t))
            .unwrap_or((trimmed, "help"));
        bot.handle_input(nick, text).await?;
    }
    Ok(())
}

async fn run_live_bot(
    manager: SessionManager<SqlitePersistence>,
    config: IrcConfig,
    _writer_guard: WriterGuard,
) -> Result<()> {
    let mut reconnect_attempts = 0u32;
    let mut backoff = ExponentialBackoff::new(&config.reconnect);

    let mut connection = connect_with_retry(&config, &mut reconnect_attempts, &mut backoff).await?;
    let transport = Arc::new(connection.transport.clone());
    let mut bot = IrcBot::new(manager, transport, config.clone());

    loop {
        match run_irc_session(&mut connection, &bot, &config).await {
            Ok(SessionEnd::Disconnected { was_registered }) => {
                if was_registered {
                    info!("IRC session ended — players remain in world; transport will reconnect");
                    backoff.reset();
                    reconnect_attempts = 0;
                } else {
                    reconnect_attempts = reconnect_attempts.saturating_add(1);
                }

                if !config.reconnect.should_retry(reconnect_attempts) {
                    if config.reconnect.enabled {
                        warn!(
                            attempts = reconnect_attempts,
                            max = config.reconnect.max_attempts,
                            "IRC reconnect limit reached — exiting"
                        );
                    } else {
                        info!("IRC disconnected — reconnect disabled (IRC_RECONNECT=false)");
                    }
                    break;
                }

                let delay = backoff.next_delay();
                warn!(
                    delay_secs = delay.as_secs(),
                    attempt = reconnect_attempts,
                    was_registered,
                    "IRC disconnected — reconnecting after backoff"
                );
                tokio::time::sleep(delay).await;

                connection =
                    connect_with_retry(&config, &mut reconnect_attempts, &mut backoff).await?;
                bot.set_transport(Arc::new(connection.transport.clone()));
            }
            Err(SessionFatal(err)) => return Err(err),
        }
    }

    Ok(())
}

async fn connect_with_retry(
    config: &IrcConfig,
    reconnect_attempts: &mut u32,
    backoff: &mut ExponentialBackoff,
) -> Result<mudl::irc::IrcConnection> {
    loop {
        match connect(config).await {
            Ok(connection) => return Ok(connection),
            Err(err) => {
                *reconnect_attempts = reconnect_attempts.saturating_add(1);
                if !config.reconnect.should_retry(*reconnect_attempts) {
                    return Err(err);
                }
                let delay = backoff.next_delay();
                warn!(
                    error = %err,
                    delay_secs = delay.as_secs(),
                    attempt = *reconnect_attempts,
                    "IRC connect failed — retrying after backoff"
                );
                tokio::time::sleep(delay).await;
            }
        }
    }
}