//! Parsed IRC wire messages used by the transport layer.

/// Direction of an IRC protocol line relative to the bot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IrcMessage {
    Ping { token: String },
    Privmsg { from: String, target: String, text: String },
    Notice { from: String, target: String, text: String },
    Join { nick: String, channel: String },
    Part { nick: String, channel: String, reason: Option<String> },
    Quit { nick: String, reason: Option<String> },
    Numeric { code: u16, params: Vec<String> },
    Raw(String),
}

/// Strip IRCv3 message tags (`@key=val;key2 ...`) from a wire line.
pub fn strip_ircv3_tags(line: &str) -> &str {
    if line.starts_with('@') {
        line.split_once(' ').map(|(_, rest)| rest).unwrap_or(line)
    } else {
        line
    }
}

/// Parse a single IRC protocol line into a structured message.
pub fn parse_irc_line(line: &str) -> IrcMessage {
    let line = line.trim_end_matches(['\r', '\n']);
    let line = strip_ircv3_tags(line);
    if let Some(rest) = line.strip_prefix("PING ") {
        return IrcMessage::Ping {
            token: rest.trim_start_matches(':').to_string(),
        };
    }

    let (prefix, payload) = if let Some(rest) = line.strip_prefix(':') {
        match rest.split_once(' ') {
            Some((pfx, body)) => (Some(pfx), body),
            None => (Some(rest), ""),
        }
    } else {
        (None, line)
    };

    let mut parts = payload.split_whitespace();
    let command = parts.next().unwrap_or_default();
    let trailing = payload
        .split_once(" :")
        .map(|(_, t)| t.to_string())
        .unwrap_or_default();
    let params: Vec<String> = parts.map(str::to_string).collect();

    match command {
        "PRIVMSG" if params.len() >= 2 => IrcMessage::Privmsg {
            from: prefix
                .and_then(extract_nick)
                .unwrap_or_else(|| "unknown".to_string()),
            target: params[0].clone(),
            text: if trailing.is_empty() {
                params[1..].join(" ")
            } else {
                trailing
            },
        },
        "NOTICE" if params.len() >= 2 => IrcMessage::Notice {
            from: prefix
                .and_then(extract_nick)
                .unwrap_or_else(|| "unknown".to_string()),
            target: params[0].clone(),
            text: if trailing.is_empty() {
                params[1..].join(" ")
            } else {
                trailing
            },
        },
        "JOIN" => {
            let channel = if !trailing.is_empty() {
                trailing
            } else {
                params.first().cloned().unwrap_or_default()
            };
            IrcMessage::Join {
                nick: prefix
                    .and_then(extract_nick)
                    .unwrap_or_else(|| "unknown".to_string()),
                channel,
            }
        }
        "PART" => {
            let channel = params.first().cloned().unwrap_or_default();
            IrcMessage::Part {
                nick: prefix
                    .and_then(extract_nick)
                    .unwrap_or_else(|| "unknown".to_string()),
                channel,
                reason: if trailing.is_empty() {
                    None
                } else {
                    Some(trailing)
                },
            }
        }
        "QUIT" => IrcMessage::Quit {
            nick: prefix
                .and_then(extract_nick)
                .unwrap_or_else(|| "unknown".to_string()),
            reason: if trailing.is_empty() {
                None
            } else {
                Some(trailing)
            },
        },
        _ if command.chars().all(|c| c.is_ascii_digit()) => IrcMessage::Numeric {
            code: command.parse().unwrap_or(0),
            params,
        },
        _ => IrcMessage::Raw(line.to_string()),
    }
}

fn extract_nick(prefix: &str) -> Option<String> {
    prefix.split('!').next().map(str::to_string)
}

/// Format an outgoing IRC command line.
pub fn format_outgoing(command: &str, params: &[&str], trailing: Option<&str>) -> String {
    let mut line = command.to_string();
    for param in params {
        line.push(' ');
        line.push_str(param);
    }
    if let Some(text) = trailing {
        line.push_str(" :");
        line.push_str(text);
    }
    line.push_str("\r\n");
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_privmsg_with_prefix() {
        let msg = parse_irc_line(":alice!u@h PRIVMSG #mudl :look");
        assert_eq!(
            msg,
            IrcMessage::Privmsg {
                from: "alice".to_string(),
                target: "#mudl".to_string(),
                text: "look".to_string(),
            }
        );
    }

    #[test]
    fn parses_ping() {
        let msg = parse_irc_line("PING :abc");
        assert_eq!(
            msg,
            IrcMessage::Ping {
                token: "abc".to_string()
            }
        );
    }

    #[test]
    fn parses_tagged_privmsg() {
        let msg = parse_irc_line(
            "@time=2026-07-07T12:00:00Z :alice!u@h PRIVMSG mudl :go north",
        );
        assert_eq!(
            msg,
            IrcMessage::Privmsg {
                from: "alice".to_string(),
                target: "mudl".to_string(),
                text: "go north".to_string(),
            }
        );
    }

    #[test]
    fn parses_privmsg_to_bot_nick() {
        let msg = parse_irc_line(":Bob!u@host PRIVMSG Mudl :login");
        assert_eq!(
            msg,
            IrcMessage::Privmsg {
                from: "Bob".to_string(),
                target: "Mudl".to_string(),
                text: "login".to_string(),
            }
        );
    }

    #[test]
    fn formats_trailing_privmsg() {
        let line = format_outgoing("PRIVMSG", &["alice"], Some("hello"));
        assert_eq!(line, "PRIVMSG alice :hello\r\n");
    }
}