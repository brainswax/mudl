//! IRCv3 capability negotiation during server registration.

use super::message::format_outgoing;

/// IRCv3 capabilities requested from the server (see https://ircv3.net).
pub const IRCV3_CAPABILITIES: &[&str] = &[
    "cap-notify",
    "server-time",
    "message-tags",
    "echo-message",
    "batch",
    "labeled-response",
    "account-tag",
];

/// Whether an incoming server line completes a `CAP * LS` multiline advertisement.
pub fn cap_ls_complete(line: &str) -> bool {
    line.contains(" CAP ") && line.contains(" LS ") && !line.contains(" LS * ")
}

/// Outgoing lines to begin IRC registration (before `CAP END`).
pub fn registration_commands(nick: &str, realname: &str, ircv3: bool) -> Vec<String> {
    let mut lines = Vec::new();
    if ircv3 {
        lines.push(format_outgoing("CAP", &["LS", "302"], None));
    }
    lines.push(format_outgoing("NICK", &[nick], None));
    lines.push(format_outgoing(
        "USER",
        &[nick, "0", "*"],
        Some(realname),
    ));
    lines
}

/// `CAP REQ` for the IRCv3 capability set.
pub fn cap_request_command() -> String {
    let caps = IRCV3_CAPABILITIES.join(" ");
    format_outgoing("CAP", &["REQ"], Some(&caps))
}

/// `CAP END` — finish capability negotiation and complete registration.
pub fn cap_end_command() -> String {
    format_outgoing("CAP", &["END"], None)
}

/// Whether the server welcomed us (RPL 001).
pub fn is_welcome(line: &str) -> bool {
    line.starts_with(':') && line.split_whitespace().nth(1) == Some("001")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_cap_ls_completion() {
        assert!(cap_ls_complete(":irc CAP * LS :multi-prefix account-notify"));
        assert!(!cap_ls_complete(":irc CAP * LS * :draft/relay-msg tags"));
    }

    #[test]
    fn registration_includes_cap_ls_when_ircv3_enabled() {
        let lines = registration_commands("mudl", "MUDL Bot", true);
        assert!(lines[0].starts_with("CAP LS"));
        assert!(lines.iter().any(|l| l.starts_with("NICK ")));
    }

    #[test]
    fn cap_request_lists_ircv3_caps() {
        let cmd = cap_request_command();
        assert!(cmd.contains("server-time"));
        assert!(cmd.contains("message-tags"));
    }
}