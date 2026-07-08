//! MUDL IRC bot — thin transport adapter over the shared session manager.
//!
//! Connects to IRCv3 servers over TLS by default (see `IRC_TLS`, `IRC_PORT`).

use std::sync::Arc;

use anyhow::Result;
use mudl::command::bootstrap_active_universe;
use mudl::gateway::SessionManager;
use mudl::irc::{
    cap_end_command, cap_ls_complete, cap_request_command, connect, is_welcome,
    registration_commands, IrcBot, IrcConfig, IrcMessage, IrcTransport, MockTransport,
};
use mudl::mudl::default_module_dir;
use mudl::object::{ObjectFactory, ObjectId};
use mudl::persistence::SqlitePersistence;
use tracing::{info, warn};

fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
}

async fn open_session_manager(config: &IrcConfig) -> Result<SessionManager<SqlitePersistence>> {
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

    SessionManager::open(persistence, anatomy)
        .await
        .map_err(Into::into)
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
    init_tracing();

    let config = IrcConfig::from_env();
    let manager = open_session_manager(&config).await?;
    info!(
        connection = %config.connection_summary(),
        nick = %config.bot_nick,
        world_channel = %config.world_channel,
        database = %config.database_url,
        "MUDL IRC bot starting"
    );

    if std::env::var("IRC_MOCK").is_ok() {
        run_mock_bot(manager, config).await
    } else {
        run_live_bot(manager, config).await
    }
}

async fn run_mock_bot(
    manager: SessionManager<SqlitePersistence>,
    config: IrcConfig,
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
) -> Result<()> {
    let mut connection = connect(&config).await?;
    let transport = Arc::new(connection.transport.clone());

    for command in registration_commands(&config.bot_nick, &config.realname, config.ircv3) {
        transport.send_raw(&command).await;
    }

    let bot = IrcBot::new(manager, Arc::clone(&transport), config.clone());
    let mut joined_world = !config.ircv3;
    let mut caps_requested = !config.ircv3;

    while let Some(line) = connection.next_line().await? {
        if config.ircv3 && cap_ls_complete(&line) && !caps_requested {
            transport.send_raw(&cap_request_command()).await;
            transport.send_raw(&cap_end_command()).await;
            caps_requested = true;
        }

        if !joined_world && is_welcome(&line) {
            transport.join(&config.world_channel).await;
            joined_world = true;
            info!(channel = %config.world_channel, "joined world channel");
        }

        let message = mudl::irc::parse_irc_line(&line);
        if let IrcMessage::Ping { token } = &message {
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