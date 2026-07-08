//! NickServ REGISTER / IDENTIFY helpers for bot startup and player relay (SEC-03).

/// Runtime NickServ settings (bot credentials and service nick).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IrcNickServConfig {
    /// NickServ service nick (usually `NickServ`).
    pub service: String,
    /// Bot account password — sent as `IDENTIFY` after server welcome.
    pub password: Option<String>,
    /// When set with `password`, bot sends `REGISTER` once before `IDENTIFY`.
    pub register_email: Option<String>,
}

impl Default for IrcNickServConfig {
    fn default() -> Self {
        Self {
            service: "NickServ".to_string(),
            password: None,
            register_email: None,
        }
    }
}

impl IrcNickServConfig {
    pub fn from_env() -> Self {
        Self {
            service: std::env::var("IRC_NICKSERV_SERVICE")
                .unwrap_or_else(|_| "NickServ".to_string()),
            password: std::env::var("IRC_NICKSERV_PASSWORD")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
            register_email: std::env::var("IRC_NICKSERV_EMAIL")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
        }
    }

    pub fn is_configured(&self) -> bool {
        self.password.is_some()
    }

    /// Outbound NickServ lines for the bot after `001` welcome.
    pub fn bot_startup_commands(&self, bot_nick: &str) -> Vec<String> {
        let Some(password) = self.password.as_deref() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        if let Some(email) = self.register_email.as_deref() {
            out.push(register_command(password, email));
        }
        out.push(identify_bot_command(password));
        let _ = bot_nick; // reserved for networks that require nick in IDENTIFY
        out
    }
}

/// `REGISTER <password> <email>` — registers the **current** connection nick (bot startup).
pub fn register_command(password: &str, email: &str) -> String {
    format!("REGISTER {password} {email}")
}

/// `IDENTIFY <password>` — identifies the current connection nick (bot startup).
pub fn identify_bot_command(password: &str) -> String {
    format!("IDENTIFY {password}")
}

/// `IDENTIFY <nick> <password>` — identify a player nick via bot relay.
pub fn identify_nick_command(nick: &str, password: &str) -> String {
    format!("IDENTIFY {nick} {password}")
}

/// Player-facing help for NickServ setup (passwords never echoed).
pub fn player_help_text(strict_identity: bool) -> String {
    let mut lines = vec![
        "NickServ commands (sent via this bot — password is not echoed back):".to_string(),
        "  nickserv identify <password>  — identify your IRC nick to the network".to_string(),
        "  nickserv register <password> <email>  — register your nick (use your IRC client)".to_string(),
        "In your IRC client you can also:".to_string(),
        "  /msg NickServ IDENTIFY <password>".to_string(),
        "  /msg NickServ REGISTER <password> <email>".to_string(),
    ];
    if strict_identity {
        lines.push(
            "This network requires an identified account before MUDL commands. \
             Identify first, then 'login'."
                .to_string(),
        );
    }
    lines.join("\n")
}

/// Instruction when relayed REGISTER is not possible from the bot connection.
pub fn register_client_instruction() -> &'static str {
    "Nick registration must be done from your own IRC connection. \
     In your client: /msg NickServ REGISTER <password> <email>"
}

/// Generic ack after relaying IDENTIFY (password not repeated).
pub fn identify_relay_ack() -> &'static str {
    "Identification request sent to NickServ. When the network confirms, try 'login' again."
}

/// Parsed NickServ feedback lines (NOTICE or PRIVMSG body).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NickServNotice {
    Identified { nick: Option<String> },
    Registered,
    AlreadyRegistered,
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
    if lower.contains("registration succeeded") || lower.contains("nickname registered") {
        return NickServNotice::Registered;
    }
    if lower.contains("is already registered") || lower.contains("already registered") {
        return NickServNotice::AlreadyRegistered;
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
    fn bot_startup_sends_register_then_identify_when_email_set() {
        let cfg = IrcNickServConfig {
            service: "NickServ".to_string(),
            password: Some("bot-pass".to_string()),
            register_email: Some("bot@example.com".to_string()),
        };
        let cmds = cfg.bot_startup_commands("mudl");
        assert_eq!(cmds.len(), 2);
        assert!(cmds[0].starts_with("REGISTER "));
        assert_eq!(cmds[1], "IDENTIFY bot-pass");
    }

    #[test]
    fn bot_startup_identify_only_without_email() {
        let cfg = IrcNickServConfig {
            service: "NickServ".to_string(),
            password: Some("bot-pass".to_string()),
            register_email: None,
        };
        let cmds = cfg.bot_startup_commands("mudl");
        assert_eq!(cmds, vec!["IDENTIFY bot-pass".to_string()]);
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
}