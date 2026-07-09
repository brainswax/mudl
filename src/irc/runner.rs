//! IRC registration and message loop for a single transport session.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use tracing::{debug, error, info, warn};

use crate::persistence::Persistence;
use crate::transport::GameTransport;

use super::bot::IrcBot;
use super::capability::{
    cap_end_command, cap_ls_complete, cap_request_command, is_nick_in_use, is_ping,
    is_registration_incomplete, is_welcome, registration_commands, registration_error_message,
};
use super::config::IrcConfig;
use super::connect::{log_outbound_command, IrcConnection};
use super::message::IrcMessage;
use super::nickserv::{parse_nickserv_reply, send_bot_nickserv_bootstrap, NickServNotice};
use super::transport::IrcTransport;

/// How a single IRC transport session ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEnd {
    /// Server closed the connection or the read stream ended.
    Disconnected {
        /// Whether the bot completed registration before the disconnect.
        was_registered: bool,
    },
}

/// Fatal registration or auth failure — reconnect will not help.
#[derive(Debug)]
pub struct SessionFatal(pub anyhow::Error);

impl SessionFatal {
    pub fn into_error(self) -> anyhow::Error {
        self.0
    }
}

impl From<anyhow::Error> for SessionFatal {
    fn from(err: anyhow::Error) -> Self {
        Self(err)
    }
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

fn log_registration_timeout(config: &IrcConfig) {
    let hint = if config.nickserv.is_configured() {
        "Check IRC_NICKSERV_PASSWORD, IRC_NICKSERV_ACCOUNT (if IRC_BOT_NICK differs), and IRC_BOT_NICK."
    } else {
        "Set IRC_NICKSERV_PASSWORD for registered-nick networks, or try IRC_IRCV3=false."
    };
    warn!(
        timeout_secs = config.registration_timeout_secs,
        hint,
        "IRC registration timed out waiting for server welcome (001) — will retry if reconnect is enabled"
    );
}

/// Register with the server and run the message loop until disconnect or fatal error.
pub async fn run_irc_session<P, T>(
    connection: &mut IrcConnection,
    bot: &IrcBot<P, T>,
    config: &IrcConfig,
) -> Result<SessionEnd, SessionFatal>
where
    P: Persistence + Clone + Send + Sync + 'static,
    T: GameTransport + IrcTransport + 'static,
{
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
            log_registration_timeout(config);
            return Ok(SessionEnd::Disconnected {
                was_registered: false,
            });
        }

        let line = match connection.next_line().await {
            Ok(line) => line,
            Err(err) => {
                warn!(error = %err, was_registered = welcomed, "IRC read failed");
                return Ok(SessionEnd::Disconnected {
                    was_registered: welcomed,
                });
            }
        };
        let Some(line) = line else {
            warn!("IRC server closed the connection");
            return Ok(SessionEnd::Disconnected {
                was_registered: welcomed,
            });
        };

        if is_ping(&line) {
            let token = line
                .trim_start()
                .strip_prefix("PING ")
                .map(|t| t.trim_start_matches(':').trim())
                .unwrap_or("");
            info!("IRC PING received — sending PONG");
            let pong = super::message::format_outgoing("PONG", &[token], None);
            log_outbound_command(&pong);
            transport.send_raw(&pong).await;
            continue;
        }

        if let Some(message) = registration_error_message(&line) {
            if is_nick_in_use(&line) {
                error!(message = %message, line = %line, "IRC registration rejected");
                return Err(SessionFatal(anyhow::anyhow!("{message}")));
            } else if is_registration_incomplete(&line) {
                debug!(
                    message = %message,
                    line = %line,
                    "IRC server deferred a command until registration completes"
                );
            } else {
                error!(message = %message, line = %line, "IRC registration rejected");
                return Err(SessionFatal(anyhow::anyhow!("{message}")));
            }
        } else if !welcomed && line.contains(" 00") {
            info!(line = %line, "IRC server message during registration");
        }

        let message = super::message::parse_irc_line(&line);
        handle_nickserv_during_registration(
            &message,
            &config.nickserv.service,
            welcomed,
        )
        .map_err(SessionFatal)?;

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
                .send_raw(&super::message::format_outgoing("PONG", &[token], None))
                .await;
        }
        if let Err(err) = bot.handle_message(message).await {
            warn!(error = %err, "IRC handler error");
        }
    }
}