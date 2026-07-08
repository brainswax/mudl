//! MUDL IRC bot — thin transport adapter over the shared session manager.
//!
//! Connects to IRCv3 servers over TLS by default (see `IRC_TLS`, `IRC_PORT`).

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use mudl::command::bootstrap_active_universe;
use mudl::gateway::SessionManager;
use mudl::irc::{
    cap_end_command, cap_ls_complete, cap_request_command, connect, is_nick_in_use, is_ping,
    is_welcome, log_outbound_command, registration_commands, registration_error_message,
    GameTransport, IrcBot, IrcConfig, IrcMessage, IrcTransport, MockTransport,
};
use mudl::mudl::default_module_dir;
use mudl::object::{ObjectFactory, ObjectId};
use mudl::persistence::{
    SqlitePersistence, WriterGuard, WriterLockOptions, WriterMode,
};
use tracing::{error, info, warn};

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

    if let Ok((_universe, loc_id)) = bootstrap_active_universe(&factory, player_id.clone()).await {
        info!(location = %loc_id, "world bootstrapped for IRC");
    } else {
        warn!("bootstrap skipped or failed — using persisted world");
    }

    SessionManager::open_with_rate_limits(persistence, anatomy, config.rate_limits.clone())
        .await
        .map_err(Into::into)
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
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
    let mut connection = connect(&config).await?;
    let transport = Arc::new(connection.transport.clone());

    let registration_deadline =
        Instant::now() + Duration::from_secs(config.registration_timeout_secs);
    info!(
        nick = %config.bot_nick,
        ircv3 = config.ircv3,
        timeout_secs = config.registration_timeout_secs,
        "sending IRC registration"
    );

    for command in registration_commands(&config.bot_nick, &config.realname, config.ircv3) {
        log_outbound_command(&command);
        transport.send_raw(&command).await;
    }

    let bot = IrcBot::new(manager, Arc::clone(&transport), config.clone());
    let mut caps_requested = !config.ircv3;
    let mut welcomed = false;

    loop {
        if !welcomed && Instant::now() >= registration_deadline {
            anyhow::bail!(
                "IRC registration timed out after {}s waiting for server welcome (001). \
                 Try IRC_IRCV3=false, a different IRC_BOT_NICK, or increase IRC_REGISTRATION_TIMEOUT.",
                config.registration_timeout_secs
            );
        }

        let line = connection
            .next_line()
            .await
            .context("IRC connection lost while waiting for server")?;
        let Some(line) = line else {
            warn!("IRC server closed the connection");
            break;
        };

        if is_ping(&line) {
            let token = line
                .trim_start()
                .strip_prefix("PING ")
                .map(|t| t.trim_start_matches(':').trim())
                .unwrap_or("");
            info!("IRC PING received — sending PONG");
            let pong = mudl::irc::format_outgoing("PONG", &[token], None);
            log_outbound_command(&pong);
            transport.send_raw(&pong).await;
            continue;
        }

        if let Some(message) = registration_error_message(&line) {
            error!(message = %message, line = %line, "IRC registration rejected");
            if is_nick_in_use(&line) {
                anyhow::bail!("{message}");
            }
        } else if !welcomed && line.contains(" 00") {
            info!(line = %line, "IRC server message during registration");
        }

        if config.ircv3 && cap_ls_complete(&line) && !caps_requested {
            info!("IRC CAP LS complete — requesting IRCv3 capabilities");
            let req = cap_request_command();
            let end = cap_end_command();
            log_outbound_command(&req);
            log_outbound_command(&end);
            transport.send_raw(&req).await;
            transport.send_raw(&end).await;
            caps_requested = true;
        }

        if !welcomed && is_welcome(&line) {
            welcomed = true;
            info!(nick = %config.bot_nick, "IRC registration complete (001 welcome)");
            bot.send_nickserv_startup().await;
            transport.join(&config.world_channel).await;
            info!(channel = %config.world_channel, "joined world channel");
        }

        if !welcomed {
            continue;
        }

        let message = mudl::irc::parse_irc_line(&line);
        if let IrcMessage::Ping { token } = &message {
            info!("IRC PING received — sending PONG");
            transport
                .send_raw(&mudl::irc::format_outgoing("PONG", &[token], None))
                .await;
        }
        if let Err(err) = bot.handle_message(message).await {
            warn!(error = %err, "IRC handler error");
        }
    }
    Ok(())
}