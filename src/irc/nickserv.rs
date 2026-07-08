//! NickServ IDENTIFY helpers for bot startup and player relay (SEC-03).
//!
//! Nick registration is a one-time manual step in your IRC client (`/msg NickServ REGISTER …`).
//! The bot only auto-IDENTIFYs on connect and relays player IDENTIFY on request.

use crate::env::read_config_secret;
use crate::transport::GameTransport;

/// Runtime NickServ settings (bot credentials and service nick).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IrcNickServConfig {
    /// NickServ service nick (usually `NickServ`).
    pub service: String,
    /// Registered NickServ account name when it differs from the connection nick.
    pub account: Option<String>,
    /// Bot account password — sent as `IDENTIFY` during IRC registration.
    pub password: Option<String>,
}

impl Default for IrcNickServConfig {
    fn default() -> Self {
        Self {
            service: "NickServ".to_string(),
            account: None,
            password: None,
        }
    }
}

impl IrcNickServConfig {
    pub fn from_env() -> Self {
        Self {
            service: std::env::var("IRC_NICKSERV_SERVICE")
                .unwrap_or_else(|_| "NickServ".to_string()),
            account: read_optional_env("IRC_NICKSERV_ACCOUNT"),
            password: read_config_secret("IRC_NICKSERV_PASSWORD"),
        }
    }

    pub fn is_configured(&self) -> bool {
        self.password.is_some()
    }

    /// Outbound NickServ line for the bot during IRC registration (before `001`).
    pub fn bot_identify_command(&self) -> Option<String> {
        let password = self.password.as_deref()?;
        Some(match self.account.as_deref() {
            Some(account) => identify_account_command(account, password),
            None => identify_bot_command(password),
        })
    }
}

fn read_optional_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// `IDENTIFY <password>` — identifies the current connection nick (bot startup).
pub fn identify_bot_command(password: &str) -> String {
    format!("IDENTIFY {password}")
}

/// `IDENTIFY <account> <password>` — identify a registered account from another nick.
pub fn identify_account_command(account: &str, password: &str) -> String {
    format!("IDENTIFY {account} {password}")
}

/// Auto-IDENTIFY during IRC registration.
///
/// On IRCv3 servers, send after `CAP END` (Ergo rejects earlier PRIVMSG with 451).
/// On legacy servers, send immediately after `NICK`/`USER`.
///
/// The NickServ account must already be registered (one-time, via IRC client).
/// When `IRC_BOT_NICK` differs from that account, set `IRC_NICKSERV_ACCOUNT`.
pub async fn send_bot_nickserv_bootstrap<T: GameTransport + ?Sized>(
    transport: &T,
    nickserv: &IrcNickServConfig,
) {
    let Some(command) = nickserv.bot_identify_command() else {
        return;
    };
    transport.send_direct(&nickserv.service, &command).await;
}

/// `IDENTIFY <nick> <password>` — identify a player nick via bot relay.
pub fn identify_nick_command(nick: &str, password: &str) -> String {
    format!("IDENTIFY {nick} {password}")
}

/// One-time manual registration — not a bot command; use your IRC client.
pub const MANUAL_REGISTRATION_HINT: &str =
    "Register your nick once in your IRC client: /msg NickServ REGISTER <password> <email>";

/// Player-facing help for NickServ (passwords never echoed).
pub fn player_help_text(strict_identity: bool) -> String {
    let mut lines = vec![
        MANUAL_REGISTRATION_HINT.to_string(),
        String::new(),
        "Bot commands (password is not echoed back):".to_string(),
        "  nickserv identify <password>  — identify your IRC nick".to_string(),
        "  nickserv help  — this summary".to_string(),
        String::new(),
        "Or in your IRC client:".to_string(),
        "  /msg NickServ IDENTIFY <password>".to_string(),
    ];
    if strict_identity {
        lines.push(String::new());
        lines.push(
            "This network requires an identified account before MUDL commands. \
             Identify first, then 'login'."
                .to_string(),
        );
    }
    lines.join("\n")
}

/// Generic ack after relaying IDENTIFY (password not repeated).
pub fn identify_relay_ack() -> &'static str {
    "Identification request sent to NickServ. When the network confirms, try 'login' again."
}

/// Parsed NickServ feedback lines (NOTICE or PRIVMSG body).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NickServNotice {
    Identified { nick: Option<String> },
    InvalidPassword,
    Other(String),
}

/// Classify common NickServ/Atheme replies for session hints.
pub fn parse_nickserv_reply(text: &str) -> NickServNotice {
    let lower = text.to_ascii_lowercase();
    if lower.contains("password accepted")
        || lower.contains("you are now identified")
        || lower.contains("successfully identified")
        || lower.contains("now recognized")
    {
        let nick = extract_quoted_nick(text);
        return NickServNotice::Identified { nick };
    }
    if lower.contains("invalid password")
        || lower.contains("authentication failed")
        || lower.contains("incorrect password")
    {
        return NickServNotice::InvalidPassword;
    }
    NickServNotice::Other(text.to_string())
}

fn extract_quoted_nick(text: &str) -> Option<String> {
    text.split('\'')
        .nth(1)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identify_nick_command_includes_nick_and_password() {
        assert_eq!(
            identify_nick_command("alice", "sekrit"),
            "IDENTIFY alice sekrit"
        );
    }

    #[test]
    fn bot_identify_command_when_password_set() {
        let cfg = IrcNickServConfig {
            service: "NickServ".to_string(),
            account: None,
            password: Some("bot-pass".to_string()),
        };
        assert_eq!(
            cfg.bot_identify_command().as_deref(),
            Some("IDENTIFY bot-pass")
        );
    }

    #[test]
    fn bot_identify_command_uses_account_when_set() {
        let cfg = IrcNickServConfig {
            service: "NickServ".to_string(),
            account: Some("muddlebot".to_string()),
            password: Some("bot-pass".to_string()),
        };
        assert_eq!(
            cfg.bot_identify_command().as_deref(),
            Some("IDENTIFY muddlebot bot-pass")
        );
    }

    #[test]
    fn identify_account_command_includes_account_and_password() {
        assert_eq!(
            identify_account_command("muddlebot", "sekrit"),
            "IDENTIFY muddlebot sekrit"
        );
    }

    #[test]
    fn bot_identify_command_none_without_password() {
        let cfg = IrcNickServConfig::default();
        assert!(cfg.bot_identify_command().is_none());
    }

    #[test]
    fn parse_identified_notice() {
        let notice = parse_nickserv_reply(
            "Password accepted - you are now recognized as user 'alice'.",
        );
        assert_eq!(
            notice,
            NickServNotice::Identified {
                nick: Some("alice".to_string())
            }
        );
    }

    #[test]
    fn parse_invalid_password() {
        let notice = parse_nickserv_reply("Invalid password.");
        assert_eq!(notice, NickServNotice::InvalidPassword);
    }

    #[test]
    fn player_help_mentions_manual_registration_not_bot_register() {
        let help = player_help_text(false);
        assert!(help.contains("IRC client"));
        assert!(help.contains("REGISTER"));
        assert!(!help.contains("nickserv register"));
    }

    #[test]
    fn from_env_loads_password_from_project_dotenv() {
        crate::env::load_project_env();
        let literal = crate::env::read_literal_dotenv_secret("IRC_NICKSERV_PASSWORD");
        if literal.is_none() {
            return;
        }
        let cfg = IrcNickServConfig::from_env();
        assert!(
            cfg.is_configured(),
            "IRC_NICKSERV_PASSWORD in .env should configure NickServ"
        );
    }

    #[tokio::test]
    async fn bootstrap_sends_only_identify_to_nickserv() {
        use crate::transport::MockTransport;

        let transport = MockTransport::new();
        let nickserv = IrcNickServConfig {
            service: "NickServ".to_string(),
            account: None,
            password: Some("bot-pass".to_string()),
        };
        send_bot_nickserv_bootstrap(&transport, &nickserv).await;
        let lines = transport.privmsgs_to("NickServ");
        assert_eq!(lines, vec!["IDENTIFY bot-pass"]);
        assert!(!lines.iter().any(|l| l.starts_with("REGISTER")));
    }
}