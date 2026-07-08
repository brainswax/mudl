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
    is_registration_incomplete, is_welcome, log_outbound_command, parse_nickserv_reply,
    registration_commands, registration_error_message, send_bot_nickserv_bootstrap, IrcBot,
    IrcConfig,
    IrcMessage, IrcTransport, MockTransport, NickServNotice,
};
use mudl::mudl::default_module_dir;
use mudl::object::{ObjectFactory, ObjectId};
use mudl::persistence::{
    SqlitePersistence, WriterGuard, WriterLockOptions, WriterMode,
};
use mudl::transport::GameTransport;
use tracing::{debug, error, info, warn};

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

fn handle_nickserv_during_registration(
    message: &IrcMessage,
    service: &str,
    welcomed: bool,
) -> Result<()> {
    if welcomed {
        return Ok(());
    }
    let (from, text) = match message {
        IrcMessage::Notice { from, text, .. } | IrcMessage::Privmsg { from, text, .. } => {
            (from.as_str(), text.as_str())
        }
        _ => return Ok(()),
    };
    if !from.eq_ignore_ascii_case(service) {
        return Ok(());
    }
    match parse_nickserv_reply(text) {
        NickServNotice::Identified { .. } => {
            info!("NickServ accepted bot credentials — awaiting server welcome (001)");
        }
        NickServNotice::InvalidPassword => {
            anyhow::bail!(
                "NickServ rejected IRC_NICKSERV_PASSWORD — fix credentials and restart"
            );
        }
        NickServNotice::Other(body) => {
            info!(response = %body, "NickServ response during registration");
        }
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
        nickserv = config.nickserv.is_configured(),
        "sending IRC registration"
    );

    for command in registration_commands(&config.bot_nick, &config.realname, config.ircv3) {
        log_outbound_command(&command);
        transport.send_raw(&command).await;
    }

    if !config.nickserv.is_configured() {
        warn!(
            "IRC_NICKSERV_PASSWORD is not set — registered-nick networks may withhold messages until identified. \
             If it is in .env, quote values containing $ (e.g. IRC_NICKSERV_PASSWORD='your$pass')"
        );
    }

    let bot = IrcBot::new(manager, Arc::clone(&transport), config.clone());
    let mut caps_requested = !config.ircv3;
    let mut nickserv_sent = !config.ircv3 && config.nickserv.is_configured();
    let mut welcomed = false;

    if nickserv_sent {
        info!(
            service = %config.nickserv.service,
            "NickServ auto-identify — sent after NICK/USER (legacy registration)"
        );
        send_bot_nickserv_bootstrap(transport.as_ref(), &config.nickserv).await;
    }

    loop {
        if !welcomed && Instant::now() >= registration_deadline {
            let hint = if config.nickserv.is_configured() {
                "Check IRC_NICKSERV_PASSWORD, IRC_NICKSERV_ACCOUNT (if IRC_BOT_NICK differs), and IRC_BOT_NICK."
            } else {
                "Set IRC_NICKSERV_PASSWORD for registered-nick networks, or try IRC_IRCV3=false."
            };
            anyhow::bail!(
                "IRC registration timed out after {}s waiting for server welcome (001). {hint}",
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
            if is_nick_in_use(&line) {
                error!(message = %message, line = %line, "IRC registration rejected");
                anyhow::bail!("{message}");
            } else if is_registration_incomplete(&line) {
                debug!(
                    message = %message,
                    line = %line,
                    "IRC server deferred a command until registration completes"
                );
            } else {
                error!(message = %message, line = %line, "IRC registration rejected");
            }
        } else if !welcomed && line.contains(" 00") {
            info!(line = %line, "IRC server message during registration");
        }

        let message = mudl::irc::parse_irc_line(&line);
        handle_nickserv_during_registration(
            &message,
            &config.nickserv.service,
            welcomed,
        )?;

        if config.ircv3 && cap_ls_complete(&line) && !caps_requested {
            info!("IRC CAP LS complete — requesting IRCv3 capabilities");
            let req = cap_request_command();
            let end = cap_end_command();
            log_outbound_command(&req);
            log_outbound_command(&end);
            transport.send_raw(&req).await;
            transport.send_raw(&end).await;
            caps_requested = true;
            if config.nickserv.is_configured() && !nickserv_sent {
                info!(
                    service = %config.nickserv.service,
                    "NickServ auto-identify — sent after CAP END"
                );
                send_bot_nickserv_bootstrap(transport.as_ref(), &config.nickserv).await;
                nickserv_sent = true;
            }
        }

        if !welcomed && is_welcome(&line) {
            welcomed = true;
            info!(nick = %config.bot_nick, "IRC registration complete (001 welcome)");
            transport.join(&config.world_channel).await;
            info!(channel = %config.world_channel, "joined world channel");
        }

        if !welcomed {
            continue;
        }

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