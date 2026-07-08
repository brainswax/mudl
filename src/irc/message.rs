//! Parsed IRC wire messages used by the transport layer.

use super::nick::sanitize_irc_nick;

/// IRCv3 message tags parsed from the wire prefix.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ircv3Tags {
    /// SASL/NickServ account name (`account-tag` capability).
    pub account: Option<String>,
}

/// Direction of an IRC protocol line relative to the bot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IrcMessage {
    Ping { token: String },
    Privmsg {
        from: String,
        account: Option<String>,
        target: String,
        text: String,
    },
    Notice {
        from: String,
        account: Option<String>,
        target: String,
        text: String,
    },
    Join {
        nick: String,
        account: Option<String>,
        channel: String,
    },
    Part {
        nick: String,
        account: Option<String>,
        channel: String,
        reason: Option<String>,
    },
    Quit {
        nick: String,
        account: Option<String>,
        reason: Option<String>,
    },
    Numeric { code: u16, params: Vec<String> },
    Raw(String),
}

/// Strip IRCv3 message tags (`@key=val;key2 ...`) from a wire line.
pub fn strip_ircv3_tags(line: &str) -> &str {
    split_ircv3_tags(line).1
}

/// Split IRCv3 tags from the remainder of a protocol line.
pub fn split_ircv3_tags(line: &str) -> (Ircv3Tags, &str) {
    if !line.starts_with('@') {
        return (Ircv3Tags::default(), line);
    }
    let Some((tag_blob, rest)) = line.split_once(' ') else {
        return (Ircv3Tags::default(), line);
    };
    (parse_tag_blob(tag_blob.trim_start_matches('@')), rest)
}

fn parse_tag_blob(blob: &str) -> Ircv3Tags {
    let mut tags = Ircv3Tags::default();
    for part in blob.split(';') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        if key.eq_ignore_ascii_case("account") {
            let value = unescape_tag_value(value);
            tags.account = if value.is_empty() {
                None
            } else {
                Some(value)
            };
        }
    }
    tags
}

fn unescape_tag_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some(':') => out.push(':'),
                Some(';') => out.push(';'),
                Some('\\') => out.push('\\'),
                Some('s') => out.push(' '),
                Some('r') => out.push('\r'),
                Some('n') => out.push('\n'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// Parse a single IRC protocol line into a structured message.
pub fn parse_irc_line(line: &str) -> IrcMessage {
    let line = line.trim_end_matches(['\r', '\n']);
    let (tags, line) = split_ircv3_tags(line);
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
    let nick = prefix.and_then(extract_nick);

    match command {
        "PRIVMSG" if params.len() >= 2 => IrcMessage::Privmsg {
            from: nick.clone().unwrap_or_else(|| "unknown".to_string()),
            account: tags.account,
            target: params[0].clone(),
            text: if trailing.is_empty() {
                params[1..].join(" ")
            } else {
                trailing
            },
        },
        "NOTICE" if params.len() >= 2 => IrcMessage::Notice {
            from: nick.clone().unwrap_or_else(|| "unknown".to_string()),
            account: tags.account,
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
                nick: nick.clone().unwrap_or_else(|| "unknown".to_string()),
                account: tags.account,
                channel,
            }
        }
        "PART" => {
            let channel = params.first().cloned().unwrap_or_default();
            IrcMessage::Part {
                nick: nick.clone().unwrap_or_else(|| "unknown".to_string()),
                account: tags.account,
                channel,
                reason: if trailing.is_empty() {
                    None
                } else {
                    Some(trailing)
                },
            }
        }
        "QUIT" => IrcMessage::Quit {
            nick: nick.unwrap_or_else(|| "unknown".to_string()),
            account: tags.account,
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
    let raw = prefix.split('!').next()?;
    sanitize_irc_nick(raw)
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
                account: None,
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
    fn parses_tagged_privmsg_with_account() {
        let msg = parse_irc_line(
            "@account=AliceAcct;time=2026-07-07T12:00:00Z :alice!u@h PRIVMSG mudl :go north",
        );
        assert_eq!(
            msg,
            IrcMessage::Privmsg {
                from: "alice".to_string(),
                account: Some("AliceAcct".to_string()),
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
                from: "bob".to_string(),
                account: None,
                target: "Mudl".to_string(),
                text: "login".to_string(),
            }
        );
    }

    #[test]
    fn invalid_nick_prefix_falls_back_to_unknown() {
        let msg = parse_irc_line(":1bad!u@h PRIVMSG #mudl :hi");
        assert_eq!(
            msg,
            IrcMessage::Privmsg {
                from: "unknown".to_string(),
                account: None,
                target: "#mudl".to_string(),
                text: "hi".to_string(),
            }
        );
    }

    #[test]
    fn parses_escaped_account_tag() {
        let (tags, _) = split_ircv3_tags("@account=Alice\\:Acct :x!u@h PRIVMSG #c :hi");
        assert_eq!(tags.account.as_deref(), Some("Alice:Acct"));
    }

    #[test]
    fn formats_trailing_privmsg() {
        let line = format_outgoing("PRIVMSG", &["alice"], Some("hello"));
        assert_eq!(line, "PRIVMSG alice :hello\r\n");
    }
}