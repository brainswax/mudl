//! Transport-agnostic player command routing (M6 prep).
//!
//! [`CommandDispatcher`] executes verbs against a [`Session`] and returns a
//! [`CommandResult`] free of IRC/Slack formatting. Transports map results to
//! their own delivery plans (`DispatchOutcome`, REPL `println!`, etc.).

use crate::command::parse::CommandLine;
use crate::command::{authorize_meta_command, authorize_plain_command};
use crate::display::{
    format_room_look_player, narrate_no_location, narrate_target_not_found, Describable,
    DisplayMode, ResolveScope, TargetResolution,
};
use crate::creature::attack_creature;
use crate::inventory::{
    close_container, describe_inventory, drop_item, open_container, take_item, InventoryError,
};
use crate::object::ObjectId;
use crate::persistence::Persistence;
use crate::repl::Session;
use crate::world::{exit_index, movement_from_line};

/// Outcome of routing one player command — transport-neutral.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommandResult {
    /// Lines shown to the actor who issued the command.
    pub lines_to_actor: Vec<String>,
    /// Room-local or private speech to fan out by the transport layer.
    pub social: Option<SocialIntent>,
    /// Movement that may require presence/channel sync (IRC/Slack).
    pub movement: Option<MovementChange>,
    /// Flush dirty world objects after this command.
    pub persist_world: bool,
}

/// Raw social payload before transport formatting (`format_say`, Slack blocks, …).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SocialIntent {
    Say {
        room_id: ObjectId,
        speaker_name: String,
        text: String,
    },
    Emote {
        room_id: ObjectId,
        speaker_name: String,
        text: String,
    },
    Tell {
        target_identity: String,
        speaker_name: String,
        text: String,
    },
}

/// Player changed rooms — transports map to JOIN/PART or thread moves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovementChange {
    pub old_room: Option<ObjectId>,
    pub new_room: Option<ObjectId>,
    pub lines: Vec<String>,
}

/// Scope and display mode for [`CommandDispatcher::look`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookOptions {
    pub scope: ResolveScope,
    pub mode: DisplayMode,
    pub brief: bool,
}

impl LookOptions {
    pub fn player(scope: ResolveScope) -> Self {
        Self {
            scope,
            mode: DisplayMode::Player,
            brief: true,
        }
    }

    pub fn builder() -> Self {
        Self {
            scope: ResolveScope::General,
            mode: DisplayMode::Builder,
            brief: false,
        }
    }
}

/// Player verb routing configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerDispatchOptions {
    pub look: LookOptions,
}

impl Default for PlayerDispatchOptions {
    fn default() -> Self {
        Self {
            look: LookOptions::player(ResolveScope::General),
        }
    }
}

/// Shared command router for REPL, IRC, and future Slack transports.
pub struct CommandDispatcher;

impl CommandDispatcher {
    /// Route a logged-in player command line against `session`.
    pub async fn dispatch_player_line<P: Persistence>(
        session: &mut Session,
        persistence: &P,
        line: &CommandLine,
        options: &PlayerDispatchOptions,
    ) -> CommandResult {
        if line.is_meta {
            return Self::meta_async(session, line).await;
        }

        match line.verb.as_str() {
            "help" | "?" => Self::irc_help(),
            "login" => Self::message(
                "You are already logged in. Send 'quit' to disconnect.".to_string(),
            ),
            "look" | "l" => {
                let target = (!line.args.is_empty()).then(|| line.args.join(" "));
                Self::look_async(session, persistence, target.as_deref(), options.look.clone())
                    .await
            }
            "inventory" | "i" => Self::inventory_async(session).await,
            "say" | "'" => Self::say_intent(session, &line.args).await,
            "emote" | ":" => Self::emote_intent(session, &line.args).await,
            "tell" | "whisper" => Self::tell_intent(session, &line.args).await,
            "take" | "get" => {
                let target = (!line.args.is_empty()).then(|| line.args.join(" "));
                Self::take_async(session, target.as_deref()).await
            }
            "drop" => {
                let target = (!line.args.is_empty()).then(|| line.args.join(" "));
                Self::drop_async(session, target.as_deref()).await
            }
            "open" => {
                let target = (!line.args.is_empty()).then(|| line.args.join(" "));
                Self::open_async(session, target.as_deref()).await
            }
            "close" => {
                let target = (!line.args.is_empty()).then(|| line.args.join(" "));
                Self::close_async(session, target.as_deref()).await
            }
            "attack" => {
                let target = (!line.args.is_empty()).then(|| line.args.join(" "));
                Self::attack_async(session, target.as_deref()).await
            }
            "go" | _ => Self::movement_async(session, line).await,
        }
    }

    pub async fn look_async<P: Persistence>(
        session: &mut Session,
        persistence: &P,
        target_name: Option<&str>,
        options: LookOptions,
    ) -> CommandResult {
        let resolution = if let Some(name) = target_name {
            session.resolve_target_async(name, options.scope).await
        } else if let Some(loc) = session.current_location() {
            TargetResolution::Found(loc.clone())
        } else {
            TargetResolution::NotFound
        };

        if let TargetResolution::Found(ref id) = resolution {
            let _ = session.ensure_object(persistence, id).await;
        }

        let mut lines = Vec::new();
        match resolution {
            TargetResolution::Found(id) => {
                let is_room = session
                    .objects_async()
                    .await
                    .get(&id)
                    .is_some_and(|obj| obj.is_location());
                let player_mode = options.mode == DisplayMode::Player;
                if is_room && player_mode {
                    let discovery = session.perceive_hidden_on_look_async().await;
                    lines.extend(discovery.lines);
                }
                let mut ctx = session.display_context_async(options.mode.clone()).await;
                if options.brief && player_mode {
                    ctx = ctx.with_flags(crate::display::DisplayFlags::BRIEF);
                }
                if let Some(obj) = ctx.objects.get(&id) {
                    if obj.is_location() && player_mode {
                        lines.push(format_room_look_player(obj, &ctx));
                    } else {
                        lines.push(obj.describe(&ctx));
                    }
                } else if let Some(name) = target_name {
                    lines.push(narrate_target_not_found(name));
                } else {
                    lines.push(narrate_no_location());
                }
            }
            TargetResolution::Ambiguous(msg) => lines.push(msg),
            TargetResolution::NotFound => {
                if let Some(name) = target_name {
                    lines.push(narrate_target_not_found(name));
                } else {
                    lines.push(narrate_no_location());
                }
            }
        }

        CommandResult {
            lines_to_actor: lines,
            ..Default::default()
        }
    }

    pub async fn inventory_async(session: &Session) -> CommandResult {
        let text = session
            .with_world_async(|world, player| {
                world
                    .object(player.actor_id())
                    .map(|obj| describe_inventory(obj, world.objects(), world.anatomy()))
                    .unwrap_or_else(|| "You seem to have lost yourself.".to_string())
            })
            .await;
        Self::message(text)
    }

    pub async fn drop_async(session: &mut Session, args: Option<&str>) -> CommandResult {
        let Some(args) = args else {
            return Self::message("Usage: drop [count] <item>".to_string());
        };
        match session
            .with_inventory_async(|ctx| drop_item(ctx, args))
            .await
        {
            Ok(msg) => CommandResult {
                lines_to_actor: vec![msg],
                persist_world: true,
                ..Default::default()
            },
            Err(InventoryError::NotFound(name)) => Self::message(narrate_target_not_found(&name)),
            Err(err) => Self::message(err.to_string()),
        }
    }

    pub async fn attack_async(session: &mut Session, target: Option<&str>) -> CommandResult {
        let Some(target) = target else {
            return Self::message("Usage: attack <creature>".to_string());
        };
        let old_room = session.current_location().cloned();
        match session
            .with_inventory_async(|ctx| {
                attack_creature(
                    ctx.dispatch,
                    ctx.player_id,
                    ctx.room_id,
                    ctx.objects,
                    ctx.anatomy,
                    ctx.dirty.as_deref_mut(),
                    target,
                )
            })
            .await
        {
            Ok(outcome) => {
                let mut result = CommandResult {
                    lines_to_actor: outcome.lines,
                    persist_world: true,
                    ..Default::default()
                };
                if let Some(new_room) = outcome.respawn_location {
                    session.set_current_location(new_room.clone());
                    result.movement = Some(MovementChange {
                        old_room,
                        new_room: Some(new_room),
                        lines: Vec::new(),
                    });
                }
                result
            }
            Err(err) => Self::message(err.to_string()),
        }
    }

    pub async fn take_async(session: &mut Session, target: Option<&str>) -> CommandResult {
        let Some(target) = target else {
            return Self::message("Take what?".to_string());
        };
        match session.with_inventory_async(|ctx| take_item(ctx, target)).await {
            Ok(msg) => CommandResult {
                lines_to_actor: vec![msg],
                persist_world: true,
                ..Default::default()
            },
            Err(InventoryError::NotFound(_)) => {
                Self::message(narrate_target_not_found(target))
            }
            Err(err) => Self::message(err.to_string()),
        }
    }

    pub async fn open_async(session: &mut Session, target: Option<&str>) -> CommandResult {
        let Some(target) = target else {
            return Self::message("Usage: open <container>".to_string());
        };
        match session
            .with_inventory_async(|ctx| open_container(ctx, target))
            .await
        {
            Ok(msg) => CommandResult {
                lines_to_actor: vec![msg],
                persist_world: true,
                ..Default::default()
            },
            Err(InventoryError::NotFound(_)) => {
                Self::message(narrate_target_not_found(target))
            }
            Err(err) => Self::message(err.to_string()),
        }
    }

    pub async fn close_async(session: &mut Session, target: Option<&str>) -> CommandResult {
        let Some(target) = target else {
            return Self::message("Usage: close <container>".to_string());
        };
        match session
            .with_inventory_async(|ctx| close_container(ctx, target))
            .await
        {
            Ok(msg) => CommandResult {
                lines_to_actor: vec![msg],
                persist_world: true,
                ..Default::default()
            },
            Err(InventoryError::NotFound(_)) => {
                Self::message(narrate_target_not_found(target))
            }
            Err(err) => Self::message(err.to_string()),
        }
    }

    pub async fn movement_async(session: &mut Session, line: &CommandLine) -> CommandResult {
        let index = session
            .with_world_async(|world, player| {
                player
                    .current_location()
                    .and_then(|loc| world.object(loc))
                    .map(exit_index)
            })
            .await;

        let arg_refs: Vec<&str> = line.args.iter().map(String::as_str).collect();
        let direction = movement_from_line(&line.verb, &arg_refs, index.as_ref());
        let Some(direction) = direction else {
            return Self::message(format!(
                "Unknown command: {}. Type 'help'.",
                line.verb
            ));
        };

        let old_room = session.current_location().cloned();
        match session.go_async(&direction).await {
            Ok(msg) => {
                let new_room = session.current_location().cloned();
                CommandResult {
                    lines_to_actor: msg.lines().map(str::to_string).collect(),
                    movement: Some(MovementChange {
                        old_room,
                        new_room,
                        lines: Vec::new(),
                    }),
                    persist_world: true,
                    ..Default::default()
                }
            }
            Err(err) => Self::message(err.to_string()),
        }
    }

    pub async fn meta_async(session: &Session, line: &CommandLine) -> CommandResult {
        let message = session
            .with_world_async(|world, player| {
                let Some(actor) = world.object(player.actor_id()) else {
                    return "You seem to have lost yourself.".to_string();
                };
                let result = if line.verb == "create" || line.verb == "load" || line.verb == "save"
                {
                    authorize_plain_command(actor, &line.verb, line.args.first().map(String::as_str))
                } else {
                    authorize_meta_command(actor, &line.verb)
                };
                match result {
                    Ok(()) => {
                        "Builder commands over IRC are not enabled yet. Use the REPL.".to_string()
                    }
                    Err(err) => err.to_string(),
                }
            })
            .await;
        Self::message(message)
    }

    pub async fn say_intent(session: &Session, args: &[String]) -> CommandResult {
        if args.is_empty() {
            return Self::message("Say what?".to_string());
        }
        let room_id = match session.current_location().cloned() {
            Some(id) => id,
            None => return Self::message(narrate_no_location()),
        };
        let speaker_name = Self::actor_display_name(session).await;
        CommandResult {
            social: Some(SocialIntent::Say {
                room_id,
                speaker_name,
                text: args.join(" "),
            }),
            ..Default::default()
        }
    }

    pub async fn emote_intent(session: &Session, args: &[String]) -> CommandResult {
        if args.is_empty() {
            return Self::message("Emote what?".to_string());
        }
        let room_id = match session.current_location().cloned() {
            Some(id) => id,
            None => return Self::message(narrate_no_location()),
        };
        let speaker_name = Self::actor_display_name(session).await;
        CommandResult {
            social: Some(SocialIntent::Emote {
                room_id,
                speaker_name,
                text: args.join(" "),
            }),
            ..Default::default()
        }
    }

    pub async fn tell_intent(session: &Session, args: &[String]) -> CommandResult {
        if args.len() < 2 {
            return Self::message("Usage: tell <player> <message>".to_string());
        }
        let speaker_name = Self::actor_display_name(session).await;
        CommandResult {
            social: Some(SocialIntent::Tell {
                target_identity: args[0].clone(),
                speaker_name,
                text: args[1..].join(" "),
            }),
            ..Default::default()
        }
    }

    pub fn irc_help() -> CommandResult {
        CommandResult {
            lines_to_actor: vec![
                "MUDL IRC commands:".to_string(),
                "  look (l) [target]   - view room or object".to_string(),
                "  go <dir>            - move (or use exit name: north, n, ...)".to_string(),
                "  inventory (i)       - list carried items".to_string(),
                "  look self           - view your character and gear".to_string(),
                "  take <item>         - pick up an item".to_string(),
                "  drop [count] <item> - drop a carried item".to_string(),
                "  open <container>    - open a container or door".to_string(),
                "  close <container>   - close a container or door".to_string(),
                "  attack <creature>   - strike a creature (turn-based combat)".to_string(),
                "  say <text>          - speak to players in your room".to_string(),
                "  emote <text>        - perform an action in your room".to_string(),
                "  tell <nick> <text>  - private message to a connected player".to_string(),
                "  quit                - save and disconnect".to_string(),
                "World channel (#mudl): speak freely for OOC (no 'say' prefix).".to_string(),
            ],
            ..Default::default()
        }
    }

    pub fn message(text: String) -> CommandResult {
        CommandResult {
            lines_to_actor: vec![text],
            ..Default::default()
        }
    }

    async fn actor_display_name(session: &Session) -> String {
        session
            .with_world_async(|world, player| {
                world
                    .object(player.actor_id())
                    .map(|obj| obj.name.clone())
                    .unwrap_or_else(|| player.actor_id().as_str().to_string())
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{Object, PermissionFlags};
    use crate::persistence::SqlitePersistence;
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

    async fn session_in_void() -> Session {
        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let room = ObjectId::new("room:void-001");
        let mut hero = bare("player:hero-001", "Alice");
        hero.location = Some(room.clone());
        let mut place = bare("room:void-001", "The Void");
        place.set_property_string("description", "A featureless void.");
        persistence.save_object(&hero).await.unwrap();
        persistence.save_object(&place).await.unwrap();
        Session::restore(
            &persistence,
            hero.id.clone(),
            Some(room),
            crate::mudl::AnatomyRegistry::default(),
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn look_without_target_describes_room() {
        let mut session = session_in_void().await;
        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let result = CommandDispatcher::look_async(
            &mut session,
            &persistence,
            None,
            LookOptions::player(ResolveScope::RoomOnly),
        )
        .await;
        assert!(result
            .lines_to_actor
            .iter()
            .any(|l| l.contains("void") || l.contains("Void")));
    }

    #[tokio::test]
    async fn inventory_lists_actor() {
        let session = session_in_void().await;
        let result = CommandDispatcher::inventory_async(&session).await;
        assert!(!result.lines_to_actor.is_empty());
    }

    #[tokio::test]
    async fn say_intent_requires_text() {
        let session = session_in_void().await;
        let result = CommandDispatcher::say_intent(&session, &[]).await;
        assert!(result.lines_to_actor.iter().any(|l| l.contains("Say what")));
    }

    #[tokio::test]
    async fn drop_requires_item_name() {
        let mut session = session_in_void().await;
        let result = CommandDispatcher::drop_async(&mut session, None).await;
        assert!(result
            .lines_to_actor
            .iter()
            .any(|l| l.contains("Usage: drop")));
    }

    #[tokio::test]
    async fn attack_requires_target() {
        let mut session = session_in_void().await;
        let result = CommandDispatcher::attack_async(&mut session, None).await;
        assert!(result
            .lines_to_actor
            .iter()
            .any(|l| l.contains("Usage: attack")));
    }
}