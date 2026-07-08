//! IRC nick validation and display-safe formatting (SEC-03).

/// Maximum IRC nick length accepted from the wire (common network limit).
pub const MAX_IRC_NICK_LEN: usize = 30;

/// Maximum OOC message body length relayed to players.
pub const MAX_OOC_TEXT_LEN: usize = 400;

/// Sanitize an IRC nick from a message prefix for session identity.
///
/// Returns a lowercase canonical nick when the wire value matches IRC nick rules;
/// rejects empty, overlong, or control-character nicks.
pub fn sanitize_irc_nick(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_IRC_NICK_LEN {
        return None;
    }
    let mut chars = trimmed.chars();
    let first = chars.next()?;
    if !is_nick_start_char(first) {
        return None;
    }
    for ch in chars {
        if !is_nick_char(ch) {
            return None;
        }
    }
    Some(trimmed.to_ascii_lowercase())
}

/// Display-safe nick for OOC and notices — strips control characters.
pub fn sanitize_nick_display(raw: &str) -> String {
    let trimmed = raw.trim();
    let safe: String = trimmed
        .chars()
        .filter(|ch| !ch.is_control() && *ch != '\n' && *ch != '\r')
        .take(MAX_IRC_NICK_LEN)
        .collect();
    if safe.is_empty() {
        "???".to_string()
    } else {
        safe
    }
}

/// Sanitize OOC body text: no control chars, single-line, length-capped.
pub fn sanitize_ooc_text(text: &str) -> String {
    let collapsed: String = text
        .chars()
        .map(|ch| {
            if ch.is_control() || ch == '\n' || ch == '\r' {
                ' '
            } else {
                ch
            }
        })
        .collect();
    let trimmed = collapsed.split_whitespace().collect::<Vec<_>>().join(" ");
    if trimmed.chars().count() <= MAX_OOC_TEXT_LEN {
        trimmed
    } else {
        let mut out: String = trimmed.chars().take(MAX_OOC_TEXT_LEN - 1).collect();
        out.push('…');
        out
    }
}

fn is_nick_start_char(ch: char) -> bool {
    ch.is_ascii_alphabetic() || matches!(ch, '[' | ']' | '\\' | '`' | '^' | '{' | '|' | '}')
}

fn is_nick_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '-' | '[' | ']' | '\\' | '`' | '^' | '{' | '|' | '}')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_irc_nick_accepts_valid_nicks() {
        assert_eq!(sanitize_irc_nick("Alice"), Some("alice".to_string()));
        assert_eq!(sanitize_irc_nick("brain-s"), Some("brain-s".to_string()));
        assert_eq!(sanitize_irc_nick("x"), Some("x".to_string()));
    }

    #[test]
    fn sanitize_irc_nick_rejects_invalid() {
        assert!(sanitize_irc_nick("").is_none());
        assert!(sanitize_irc_nick("123bad").is_none());
        assert!(sanitize_irc_nick("al\nce").is_none());
        assert!(sanitize_irc_nick(&"a".repeat(31)).is_none());
    }

    #[test]
    fn sanitize_nick_display_strips_control_chars() {
        assert_eq!(sanitize_nick_display("Alice"), "Alice");
        assert_eq!(sanitize_nick_display("bad\nnick"), "badnick");
        assert_eq!(sanitize_nick_display("\x01"), "???");
    }

    #[test]
    fn sanitize_ooc_text_collapses_and_caps_length() {
        assert_eq!(sanitize_ooc_text("brb"), "brb");
        assert_eq!(sanitize_ooc_text("line1\nline2"), "line1 line2");
        let long = "a".repeat(500);
        let capped = sanitize_ooc_text(&long);
        assert!(capped.ends_with('…'));
        assert!(capped.chars().count() <= MAX_OOC_TEXT_LEN);
    }
}