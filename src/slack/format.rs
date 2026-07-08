//! Slack output formatting — mrkdwn, Block Kit, and readability for DMs/channels.
//!
//! Game commands produce transport-neutral plain text; this module adapts it for
//! Slack's `mrkdwn` and optional `blocks` before Web API delivery.

use serde_json::{json, Value};

use super::presence::{parse_recipient, SlackRecipient};

/// Where a line is headed — drives typography and block layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlackOutputKind {
    /// Command responses and private tells in a DM.
    DirectMessage,
    /// Ephemeral policy / error notices.
    Notice,
    /// `say` / `emote` in a room channel or thread.
    InCharacter,
    /// World-channel OOC.
    Ooc,
    /// Join hints, welcome, goodbye.
    System,
}

/// A message ready for `chat.postMessage` / `chat.postEphemeral`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackFormattedMessage {
    /// Notification preview and accessibility fallback (required by Slack).
    pub text: String,
    /// Optional Block Kit payload for multi-section output.
    pub blocks: Option<Vec<Value>>,
}

impl SlackFormattedMessage {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            blocks: None,
        }
    }

    pub fn with_blocks(text: impl Into<String>, blocks: Vec<Value>) -> Self {
        Self {
            text: text.into(),
            blocks: Some(blocks),
        }
    }
}

/// Escape user-controlled text for Slack `mrkdwn`.
pub fn escape_mrkdwn(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Classify output from recipient + raw game text.
pub fn classify_slack_output(recipient: &str, text: &str) -> SlackOutputKind {
    match parse_recipient(recipient) {
        SlackRecipient::Notice { .. } => SlackOutputKind::Notice,
        SlackRecipient::Channel { thread_ts: Some(_), .. } | SlackRecipient::ChannelName(_) => {
            SlackOutputKind::InCharacter
        }
        _ if is_ooc_line(text) => SlackOutputKind::Ooc,
        _ if is_in_character_line(text) => SlackOutputKind::InCharacter,
        _ if is_system_line(text) => SlackOutputKind::System,
        _ if recipient.starts_with('D') => SlackOutputKind::DirectMessage,
        _ => SlackOutputKind::DirectMessage,
    }
}

/// Adapt one game output line (or multi-line blob) for Slack delivery.
pub fn format_slack_message(text: &str, kind: SlackOutputKind) -> SlackFormattedMessage {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return SlackFormattedMessage::plain("");
    }

    if is_room_look(trimmed) {
        return format_room_look_slack(trimmed);
    }

    let formatted = if is_private_tell_line(trimmed) {
        format_tell_line(trimmed)
    } else {
        match kind {
            SlackOutputKind::InCharacter => format_in_character_line(trimmed),
            SlackOutputKind::Ooc => format_ooc_line(trimmed),
            SlackOutputKind::Notice => format_notice_line(trimmed),
            SlackOutputKind::System => format_system_line(trimmed),
            SlackOutputKind::DirectMessage => format_direct_line(trimmed),
        }
    };

    SlackFormattedMessage::plain(formatted)
}

// --- Social (used from dispatch) ---

/// In-character speech for room channels / threads.
pub fn format_say(speaker: &str, text: &str) -> String {
    format!(
        "*{}* says, “{}”",
        escape_mrkdwn(speaker.trim()),
        escape_mrkdwn(text.trim())
    )
}

/// In-character emote for room channels / threads.
pub fn format_emote(speaker: &str, text: &str) -> String {
    format!(
        "*{}* _{}_",
        escape_mrkdwn(speaker.trim()),
        escape_mrkdwn(text.trim())
    )
}

/// Private tell received by the target player.
pub fn format_tell(from: &str, text: &str) -> String {
    format!(
        ":envelope: *{}* whispers, “{}”",
        escape_mrkdwn(from.trim()),
        escape_mrkdwn(text.trim())
    )
}

/// Movement arrival notice for co-located players and room channels.
pub fn format_arrival(speaker: &str) -> String {
    format!("*{}* has arrived.", escape_mrkdwn(speaker.trim()))
}

/// Movement departure notice for co-located players and room channels.
pub fn format_departure(speaker: &str) -> String {
    format!("*{}* has left.", escape_mrkdwn(speaker.trim()))
}

/// Confirmation shown to the tell sender.
pub fn format_tell_sent(to: &str, text: &str) -> String {
    format!(
        "You whisper to *{}*, “{}”",
        escape_mrkdwn(to.trim()),
        escape_mrkdwn(text.trim())
    )
}

/// Out-of-character speech on the world channel.
pub fn format_ooc(speaker: &str, text: &str) -> String {
    let speaker = escape_mrkdwn(speaker.trim());
    let body = escape_mrkdwn(&sanitize_inline(text));
    format!("`[OOC]` *{speaker}:* {body}")
}

/// Multi-line `help` output as a single mrkdwn section.
pub fn format_help_text(lines: &[String]) -> SlackFormattedMessage {
    let body: Vec<String> = lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("MUDL Slack commands") {
                format!("*{}*", escape_mrkdwn(trimmed))
            } else if line.starts_with("  ") {
                format!("- {}", escape_mrkdwn(trimmed))
            } else {
                format!("_{}_", escape_mrkdwn(trimmed))
            }
        })
        .collect();

    let text = body.join("\n");
    let blocks = vec![section_block(&text)];
    SlackFormattedMessage::with_blocks(text.clone(), blocks)
}

// --- Internal helpers ---

fn is_ooc_line(text: &str) -> bool {
    let t = text.trim();
    t.starts_with("[OOC]") || t.starts_with("`[OOC]`")
}

fn is_in_character_line(text: &str) -> bool {
    (text.contains(" says, \"") || text.contains(" says, “"))
        && !text.contains(" tells you, ")
        && !text.contains(" whispers, ")
}

fn is_private_tell_line(text: &str) -> bool {
    text.contains(" tells you, ")
        || text.contains(" whispers, ")
        || text.contains("You whisper to ")
        || text.contains("You tell ")
}

fn is_system_line(text: &str) -> bool {
    let t = text.trim();
    t.starts_with("Welcome to MUDL")
        || t.starts_with("Goodbye")
        || t.starts_with("Follow ")
        || t.contains("entered the location")
        || t.contains("left the location")
}

fn is_room_look(text: &str) -> bool {
    if text == "It is pitch black." {
        return true;
    }
    text.lines().count() >= 2
        && (text.contains("Obvious exits:")
            || text.contains("You see ")
            || text.starts_with("Inside ")
            || text.contains("Through the "))
}

fn sanitize_inline(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn format_ooc_line(text: &str) -> String {
    if text.starts_with("`[OOC]`") {
        return text.to_string();
    }
    if let Some(rest) = text.strip_prefix("[OOC] ") {
        if let Some((speaker, body)) = rest.split_once(": ") {
            return format_ooc(speaker, body);
        }
    }
    escape_mrkdwn(text)
}

fn format_in_character_line(text: &str) -> String {
    if let Some((speaker, speech)) = parse_say_line(text) {
        return format_say(speaker, speech);
    }
    if let Some((speaker, action)) = parse_tell_line(text) {
        return format_tell(speaker, action);
    }
    if let Some((speaker, action)) = parse_emote_line(text) {
        return format_emote(speaker, action);
    }
    format_direct_line(text)
}

fn parse_say_line(text: &str) -> Option<(&str, &str)> {
    let (speaker, rest) = text.split_once(" says, ")?;
    let speech = rest.trim_matches('"').trim_matches('“').trim_matches('”');
    Some((speaker, speech))
}

fn parse_tell_line(text: &str) -> Option<(&str, &str)> {
    if let Some((speaker, rest)) = text.split_once(" tells you, ") {
        let speech = rest.trim_matches('"').trim_matches('“').trim_matches('”');
        return Some((speaker, speech));
    }
    if let Some((speaker, rest)) = text.split_once(" whispers, ") {
        let speech = rest.trim_matches('"').trim_matches('“').trim_matches('”');
        return Some((speaker, speech));
    }
    None
}

fn format_tell_line(text: &str) -> String {
    if text.contains(":envelope:") || text.contains(" whispers, ") {
        return text.to_string();
    }
    if let Some((speaker, speech)) = parse_tell_line(text) {
        return format_tell(speaker, speech);
    }
    if let Some(rest) = text.strip_prefix("You tell ") {
        return format!("You whisper to *{}*", escape_mrkdwn(rest));
    }
    escape_mrkdwn(text)
}

fn parse_emote_line(text: &str) -> Option<(&str, &str)> {
    let (speaker, action) = text.split_once(' ')?;
    if text.contains(" says, ") || text.contains(" tells you, ") {
        return None;
    }
    Some((speaker, action))
}

fn format_notice_line(text: &str) -> String {
    if text.contains("not logged in") {
        return format!(":warning: {}", escape_mrkdwn(text));
    }
    escape_mrkdwn(text)
}

fn format_system_line(text: &str) -> String {
    if text.starts_with("Welcome to MUDL") {
        return format!(":wave: {}", escape_mrkdwn(text));
    }
    if text.starts_with("Goodbye") {
        return format!("_{}_", escape_mrkdwn(text));
    }
    if text.starts_with("Follow ") {
        return format!("_{}_", escape_mrkdwn(text));
    }
    escape_mrkdwn(text)
}

fn format_direct_line(text: &str) -> String {
    if text.starts_with("Obvious exits:") {
        let rest = text.trim_start_matches("Obvious exits:").trim();
        return format!("*Obvious exits:* {}", escape_mrkdwn(rest));
    }
    if text.starts_with("You see ") && text.ends_with(" here.") {
        return format!("_{}_", escape_mrkdwn(text));
    }
    escape_mrkdwn(text)
}

fn format_room_look_slack(text: &str) -> SlackFormattedMessage {
    let mut blocks = Vec::new();
    let mut fallback = Vec::new();

    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let section = format_room_look_section(line);
        fallback.push(section.clone());
        blocks.push(section_block(&section));
        if line.starts_with("Obvious exits:") || line.starts_with("You see ") {
            blocks.push(json!({ "type": "divider" }));
        }
    }

    // Drop trailing divider if present.
    if blocks.last().is_some_and(|b| b.get("type") == Some(&json!("divider"))) {
        blocks.pop();
    }

    let fallback_text = fallback.join("\n");
    if blocks.len() <= 1 {
        SlackFormattedMessage::plain(fallback_text)
    } else {
        SlackFormattedMessage::with_blocks(fallback_text, blocks)
    }
}

fn format_room_look_section(line: &str) -> String {
    if line.starts_with("Obvious exits:") {
        let rest = line.trim_start_matches("Obvious exits:").trim();
        return format!("*Obvious exits:* {}", escape_mrkdwn(rest));
    }
    if line.starts_with("You see ") {
        return format!("_{}_", escape_mrkdwn(line));
    }
    if line.starts_with("Inside ") {
        return format!("*{}*", escape_mrkdwn(line.trim_end_matches('.')));
    }
    if line.starts_with("Through the ") {
        return format!("_{}_", escape_mrkdwn(line));
    }
    escape_mrkdwn(line)
}

fn section_block(mrkdwn: &str) -> Value {
    json!({
        "type": "section",
        "text": {
            "type": "mrkdwn",
            "text": mrkdwn
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_mrkdwn_protects_special_chars() {
        assert_eq!(escape_mrkdwn("a < b & c > d"), "a &lt; b &amp; c &gt; d");
    }

    #[test]
    fn format_say_uses_bold_speaker() {
        assert_eq!(format_say("Alice", "hi"), "*Alice* says, “hi”");
    }

    #[test]
    fn format_emote_uses_italic_action() {
        assert_eq!(format_emote("Alice", "waves."), "*Alice* _waves._");
    }

    #[test]
    fn format_ooc_uses_code_label() {
        assert_eq!(format_ooc("Alice", "brb"), "`[OOC]` *Alice:* brb");
    }

    #[test]
    fn format_tell_uses_envelope_emoji() {
        let line = format_tell("Bob", "psst");
        assert!(line.contains(":envelope:"));
        assert!(line.contains("*Bob*"));
    }

    #[test]
    fn room_look_produces_blocks_for_multi_section() {
        let text = "A dark void.\nObvious exits: north\nYou see a sword here.";
        let msg = format_slack_message(text, SlackOutputKind::DirectMessage);
        assert!(msg.blocks.is_some());
        assert!(msg.text.contains("Obvious exits"));
        let blocks = msg.blocks.unwrap();
        assert!(blocks.iter().any(|b| b["type"] == "section"));
    }

    #[test]
    fn single_line_look_stays_plain() {
        let msg = format_slack_message("It is pitch black.", SlackOutputKind::DirectMessage);
        assert!(msg.blocks.is_none());
    }

    #[test]
    fn classifies_thread_recipient_as_in_character() {
        assert_eq!(
            classify_slack_output("C1:thread:room-void", "Alice says, \"hi\""),
            SlackOutputKind::InCharacter
        );
    }

    #[test]
    fn help_text_uses_bullets() {
        let msg = format_help_text(&[
            "MUDL Slack commands:".to_string(),
            "  look - view room".to_string(),
        ]);
        assert!(msg.text.contains("- look"));
        assert!(msg.blocks.is_some());
    }

    #[test]
    fn upgrades_legacy_irc_say_to_slack() {
        let msg = format_slack_message(
            "Alice says, \"hello\"",
            SlackOutputKind::InCharacter,
        );
        assert_eq!(msg.text, "*Alice* says, “hello”");
    }
}