//! [`GameTransport`](crate::transport::GameTransport) recipient encoding for Slack.
//!
//! IRC uses nick names and `#channel` strings; Slack uses channel/conversation IDs,
//! optional thread timestamps, and ephemeral `channel:notice:user` tuples.

/// Parsed Slack delivery target from a transport-neutral recipient string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlackRecipient {
    /// `C…` / `D…` channel or DM conversation.
    Channel {
        id: String,
        thread_ts: Option<String>,
    },
    /// Ephemeral notice — `channel:notice:user` or legacy `channel:user`.
    Notice { channel: String, user: String },
    /// Direct message by opening a DM with `U…`.
    User { id: String },
    /// Named channel slug (e.g. `mudl-void-001`) resolved via `conversations.join`.
    ChannelName(String),
}

const NOTICE_MARKER: &str = ":notice:";
const THREAD_MARKER: &str = ":thread:";

/// Build a thread presence key for [`GameTransport::send_direct`].
pub fn encode_thread(channel_id: &str, thread_ts: &str) -> String {
    format!("{channel_id}{THREAD_MARKER}{thread_ts}")
}

/// Build an ephemeral notice recipient for [`GameTransport::send_notice`].
pub fn encode_notice(channel_id: &str, user_id: &str) -> String {
    format!("{channel_id}{NOTICE_MARKER}{user_id}")
}

/// Parse a [`GameTransport`] recipient into a Slack-specific target.
pub fn parse_recipient(recipient: &str) -> SlackRecipient {
    let trimmed = recipient.trim();
    if trimmed.is_empty() {
        return SlackRecipient::Channel {
            id: String::new(),
            thread_ts: None,
        };
    }

    if let Some((channel, user)) = trimmed.split_once(NOTICE_MARKER) {
        return SlackRecipient::Notice {
            channel: channel.to_string(),
            user: user.to_string(),
        };
    }

    if let Some((channel, thread_ts)) = trimmed.split_once(THREAD_MARKER) {
        return SlackRecipient::Channel {
            id: channel.to_string(),
            thread_ts: Some(thread_ts.to_string()),
        };
    }

    if let Some((channel, user)) = trimmed.split_once(':') {
        if channel.starts_with('C') || channel.starts_with('D') || channel.starts_with('G') {
            if user.starts_with('U') || user.starts_with('W') {
                return SlackRecipient::Notice {
                    channel: channel.to_string(),
                    user: user.to_string(),
                };
            }
        }
    }

    if trimmed.starts_with('U') || trimmed.starts_with('W') {
        return SlackRecipient::User {
            id: trimmed.to_string(),
        };
    }

    if trimmed.starts_with('C') || trimmed.starts_with('D') || trimmed.starts_with('G') {
        return SlackRecipient::Channel {
            id: trimmed.to_string(),
            thread_ts: None,
        };
    }

    SlackRecipient::ChannelName(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_channel_and_thread_targets() {
        assert_eq!(
            parse_recipient("C123"),
            SlackRecipient::Channel {
                id: "C123".to_string(),
                thread_ts: None
            }
        );
        assert_eq!(
            parse_recipient("C123:thread:111.222"),
            SlackRecipient::Channel {
                id: "C123".to_string(),
                thread_ts: Some("111.222".to_string())
            }
        );
    }

    #[test]
    fn parses_notice_targets() {
        assert_eq!(
            parse_recipient("C123:notice:U456"),
            SlackRecipient::Notice {
                channel: "C123".to_string(),
                user: "U456".to_string()
            }
        );
        assert_eq!(
            parse_recipient("C123:U456"),
            SlackRecipient::Notice {
                channel: "C123".to_string(),
                user: "U456".to_string()
            }
        );
    }

    #[test]
    fn parses_user_dm_target() {
        assert_eq!(
            parse_recipient("U789"),
            SlackRecipient::User {
                id: "U789".to_string()
            }
        );
    }

    #[test]
    fn parses_named_room_channel() {
        assert_eq!(
            parse_recipient("mudl-void-001"),
            SlackRecipient::ChannelName("mudl-void-001".to_string())
        );
    }
}