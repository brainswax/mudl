//! In-character and out-of-character social message formatting.

use super::nick::{sanitize_nick_display, sanitize_ooc_text};

/// In-character speech visible to co-located players.
pub fn format_say(speaker: &str, text: &str) -> String {
    format!("{speaker} says, \"{text}\"")
}

/// In-character emote visible to co-located players.
pub fn format_emote(speaker: &str, text: &str) -> String {
    format!("{speaker} {text}")
}

/// Private tell between two players.
pub fn format_tell(from: &str, text: &str) -> String {
    format!("{from} tells you, \"{text}\"")
}

/// Confirmation shown to the tell sender.
pub fn format_tell_sent(to: &str, text: &str) -> String {
    format!("You tell {to}, \"{text}\"")
}

/// Out-of-character speech on the world channel.
///
/// Speaker and body are sanitized — no embedded newlines or control characters.
pub fn format_ooc(speaker: &str, text: &str) -> String {
    let speaker = sanitize_nick_display(speaker);
    let text = sanitize_ooc_text(text);
    format!("[OOC] {speaker}: {text}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn social_formats_are_immersive() {
        assert_eq!(format_say("Alice", "hi"), "Alice says, \"hi\"");
        assert_eq!(format_emote("Alice", "waves."), "Alice waves.");
        assert_eq!(format_tell("Bob", "psst"), "Bob tells you, \"psst\"");
        assert_eq!(format_ooc("Alice", "brb"), "[OOC] Alice: brb");
    }

    #[test]
    fn format_ooc_sanitizes_speaker_and_body() {
        assert_eq!(format_ooc("Al\nice", "brb\nsoon"), "[OOC] Alice: brb soon");
    }
}