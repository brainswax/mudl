//! MUDL `@trigger` definitions — attach scripted events to places and objects.

/// A single trigger script bound to an event name (`on_enter`, `on_break`, etc.).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerDef {
    pub event: String,
    pub code: String,
}

/// Parse `@trigger <event> <action> [text...]` or `@trigger <event> <full script>`.
pub fn parse_trigger_line(rest: &str) -> Option<TriggerDef> {
    let rest = rest.trim();
    if rest.is_empty() {
        return None;
    }
    let mut parts = rest.splitn(2, char::is_whitespace);
    let event = parts.next()?.trim().to_lowercase();
    let code = parts.next()?.trim().to_string();
    if event.is_empty() || code.is_empty() {
        return None;
    }
    Some(TriggerDef { event, code })
}

/// Well-known world events builders can attach triggers to.
pub mod events {
    pub const ON_ENTER: &str = "on_enter";
    pub const ON_LEAVE: &str = "on_leave";
    pub const ON_TAKE: &str = "on_take";
    pub const ON_DROP: &str = "on_drop";
    pub const ON_MOVE: &str = "on_move";
    pub const ON_BREAK: &str = "on_break";
    pub const ON_DEATH: &str = "on_death";
    pub const ON_KILL: &str = "on_kill";
    pub const ON_DISCOVERED: &str = "on_discovered";
    pub const ON_UNLOCK: &str = "on_unlock";
    pub const ON_OPEN: &str = "on_open";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_trigger_narrate_and_emote() {
        let t = parse_trigger_line("on_enter narrate Silver mist clings to the branches.").unwrap();
        assert_eq!(t.event, "on_enter");
        assert_eq!(t.code, "narrate Silver mist clings to the branches.");

        let e = parse_trigger_line("on_break emote shatters into pale dust.").unwrap();
        assert_eq!(e.event, "on_break");
        assert_eq!(e.code, "emote shatters into pale dust.");
    }
}