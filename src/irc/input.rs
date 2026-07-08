//! IRC command input normalization — client prefixes and nick canonicalization.

/// Strip common IRC client prefixes so pasted commands work in mock mode and tests.
///
/// Accepts `/msg mudl look`, `/query mudl inventory`, and plain `look`.
pub fn normalize_irc_command_input(input: &str, bot_nick: &str) -> String {
    let trimmed = input.trim();
    let Some(rest) = trimmed.strip_prefix('/') else {
        return trimmed.to_string();
    };

    let mut parts = rest.split_whitespace();
    let client_verb = parts.next().unwrap_or_default();
    if !client_verb.eq_ignore_ascii_case("msg") && !client_verb.eq_ignore_ascii_case("query") {
        return trimmed.to_string();
    }

    let Some(target) = parts.next() else {
        return trimmed.to_string();
    };
    if !target.eq_ignore_ascii_case(bot_nick) {
        return trimmed.to_string();
    }

    parts.collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_msg_prefix_to_bot() {
        assert_eq!(
            normalize_irc_command_input("/msg mudl look", "mudl"),
            "look"
        );
        assert_eq!(
            normalize_irc_command_input("/MSG Mudl say hello there", "mudl"),
            "say hello there"
        );
    }

    #[test]
    fn strips_query_prefix_to_bot() {
        assert_eq!(
            normalize_irc_command_input("/query mudl help", "mudl"),
            "help"
        );
    }

    #[test]
    fn leaves_plain_commands_unchanged() {
        assert_eq!(normalize_irc_command_input("go north", "mudl"), "go north");
        assert_eq!(
            normalize_irc_command_input("/msg alice hi", "mudl"),
            "/msg alice hi"
        );
    }

    #[test]
    fn leaves_msg_to_other_target_unchanged() {
        assert_eq!(
            normalize_irc_command_input("/msg alice psst", "mudl"),
            "/msg alice psst"
        );
    }
}