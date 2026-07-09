//! Player input classification — distinguish game commands from free chat.

use crate::world::{exit_index, movement_from_line, ExitIndex};

use super::parse::CommandLine;

/// Verbs accepted while logged out over a shared open channel.
pub fn is_logged_out_channel_verb(verb: &str) -> bool {
    matches!(
        verb,
        "login" | "register" | "help" | "?" | "nickserv" | "ns" | "identify"
    )
}

/// Known logged-in player verbs (excludes room-specific movement aliases).
pub fn is_known_player_verb(verb: &str) -> bool {
    matches!(
        verb,
        "help" | "?"
            | "look"
            | "l"
            | "inventory"
            | "i"
            | "say"
            | "'"
            | "emote"
            | ":"
            | "tell"
            | "whisper"
            | "take"
            | "get"
            | "drop"
            | "open"
            | "close"
            | "attack"
            | "go"
            | "quit"
            | "logout"
            | "exit"
    )
}

/// Whether a parsed line is a recognized logged-in player command.
pub fn is_recognized_player_command(line: &CommandLine, exit_index: Option<&ExitIndex>) -> bool {
    if line.verb.is_empty() || line.is_meta {
        return false;
    }
    if is_known_player_verb(&line.verb) {
        return true;
    }
    if !line.args.is_empty() {
        return false;
    }
    movement_from_line(&line.verb, &[], exit_index).is_some()
}

/// Whether a logged-out line should be dispatched when [`LoginAuthPolicy::auto_login`] is enabled.
///
/// Used by open-channel routing so the first player command is not dropped before
/// [`attempt_auto_login`](crate::gateway::attempt_auto_login) runs in dispatch.
pub fn is_auto_login_channel_command(line: &CommandLine) -> bool {
    is_known_player_verb(&line.verb) || is_recognized_player_command(line, None)
}

/// Whether an open-channel line should run through command dispatch (vs. plain chat).
pub fn is_open_channel_game_command(
    line: &CommandLine,
    logged_in: bool,
    exit_index: Option<&ExitIndex>,
    auto_login: bool,
) -> bool {
    if line.verb.is_empty() || line.is_meta {
        return false;
    }
    if !logged_in {
        if auto_login && is_auto_login_channel_command(line) {
            return true;
        }
        return is_logged_out_channel_verb(&line.verb);
    }
    is_recognized_player_command(line, exit_index)
}

/// Build the exit index for a connected player's current room (sync, under session lock).
pub fn exit_index_for_current_room(
    world: &crate::world::WorldState,
    player: &crate::repl::PlayerSession,
) -> Option<ExitIndex> {
    player
        .current_location()
        .and_then(|loc| world.object(loc))
        .map(exit_index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::parse_command_line;
    use crate::object::{Object, ObjectId, PermissionFlags};
    use std::collections::HashMap;

    fn bare(id: &str, name: &str) -> Object {
        Object {
            id: ObjectId::new(id),
            name: name.to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: ObjectId::new("player:hero-001"),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            revision: 0,
            updated_at: None,
            is_deleted: false,
            deleted_at: None,
        }
    }

    #[test]
    fn free_chat_is_not_a_game_command() {
        let line = parse_command_line("hello everyone");
        assert!(!is_recognized_player_command(&line, None));
        assert!(!is_open_channel_game_command(&line, true, None, false));
    }

    #[test]
    fn look_is_a_game_command() {
        let line = parse_command_line("look sword");
        assert!(is_recognized_player_command(&line, None));
    }

    #[test]
    fn look_self_and_open_are_game_commands() {
        let look_self = parse_command_line("look self");
        assert!(is_recognized_player_command(&look_self, None));
        assert!(is_open_channel_game_command(&look_self, true, None, false));

        let open = parse_command_line("open chest");
        assert!(is_recognized_player_command(&open, None));
        assert!(is_open_channel_game_command(&open, true, None, false));
    }

    #[test]
    fn standalone_exit_matches_room_index() {
        let mut place = bare("room:void-001", "Void");
        place.set_property_map(
            "exits",
            HashMap::from([("north".to_string(), ObjectId::new("room:north-001"))]),
        );
        let index = exit_index(&place);
        let line = parse_command_line("north");
        assert!(is_recognized_player_command(&line, Some(&index)));
    }

    #[test]
    fn unknown_exit_without_index_is_chat() {
        let line = parse_command_line("north");
        assert!(!is_recognized_player_command(&line, None));
    }

    #[test]
    fn logged_out_login_is_recognized() {
        let line = parse_command_line("login");
        assert!(is_open_channel_game_command(&line, false, None, false));
    }

    #[test]
    fn logged_out_chatter_is_not_recognized() {
        let line = parse_command_line("hey there");
        assert!(!is_open_channel_game_command(&line, false, None, false));
    }

    #[test]
    fn auto_login_recognizes_player_commands_while_logged_out() {
        let look = parse_command_line("look");
        assert!(is_auto_login_channel_command(&look));
        assert!(is_open_channel_game_command(&look, false, None, true));

        let chat = parse_command_line("hey everyone");
        assert!(!is_auto_login_channel_command(&chat));
        assert!(!is_open_channel_game_command(&chat, false, None, true));
    }
}