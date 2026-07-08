//! Slack command input normalization — bot mentions and client prefixes.

/// Strip Slack bot mentions and common client prefixes from inbound text.
///
/// Accepts `<@U123> look`, `<@A123> help`, and plain `look`.
pub fn normalize_slack_command_input(input: &str, app_id: Option<&str>) -> String {
    let mut trimmed = input.trim().to_string();
    if trimmed.is_empty() {
        return trimmed;
    }

    if let Some(app_id) = app_id {
        let mention = format!("<@{app_id}>");
        if let Some(rest) = trimmed.strip_prefix(&mention) {
            trimmed = rest.trim().to_string();
        }
    }

    // Strip a leading user mention when players @-mention the bot by user id.
    if trimmed.starts_with("<@") {
        if let Some(end) = trimmed.find('>') {
            trimmed = trimmed[end + 1..].trim().to_string();
        }
    }

    trimmed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_app_mention_prefix() {
        assert_eq!(
            normalize_slack_command_input("<@A123> look", Some("A123")),
            "look"
        );
    }

    #[test]
    fn strips_user_mention_prefix_without_app_id() {
        assert_eq!(
            normalize_slack_command_input("<@U999> say hello", None),
            "say hello"
        );
    }

    #[test]
    fn leaves_plain_commands_unchanged() {
        assert_eq!(normalize_slack_command_input("go north", None), "go north");
    }
}