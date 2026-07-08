//! Command dispatch from IRC input to [`Session`] operations.

use std::sync::Arc;

use tokio::sync::Mutex;

use crate::command::parse_command_line;
use crate::command::{authorize_meta_command, authorize_plain_command, CommandLine};
use crate::display::{
    format_room_look_player, narrate_no_location, narrate_target_not_found, Describable,
    DisplayMode, ResolveScope, TargetResolution,
};
use crate::gateway::{normalize_nick, SessionManager};
use crate::inventory::{describe_inventory, take_item, InventoryError};
use crate::object::{Object, ObjectId};
use crate::persistence::Persistence;
use crate::world::{exit_index, movement_from_line};

use super::channels::{room_channel_name, room_join_notice};
use super::config::IrcConfig;
use super::social::{format_emote, format_say, format_tell, format_tell_sent};
use super::visibility::{
    actor_display_name_async, players_in_room_async, resolve_connected_nick,
};

/// IRC routing instructions produced by command dispatch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DispatchOutcome {
    pub sender: String,
    pub to_sender: Vec<String>,
    pub private: Vec<(String, String)>,
    pub room_audience: Vec<RoomDelivery>,
    pub channel: Vec<(String, String)>,
    pub channel_sync: Option<ChannelSync>,
    pub persist: bool,
}

/// Lines delivered to co-located players (excluding the speaker).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomDelivery {
    pub audience: Vec<String>,
    pub lines: Vec<String>,
}

/// Join/part instructions for room channel membership.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelSync {
    pub nick: String,
    pub join: Vec<String>,
    pub part: Vec<String>,
}

/// Resolve a player object for `login` from an explicit id or nick-matched name.
pub fn resolve_player_for_login(
    nick: &str,
    explicit: Option<&str>,
    objects: &std::collections::HashMap<ObjectId, Object>,
) -> Option<ObjectId> {
    if let Some(raw) = explicit {
        let id = ObjectId::new(raw);
        if objects.get(&id).is_some() {
            return Some(id);
        }
        return None;
    }

    objects
        .values()
        .filter(|obj| obj.id.as_str().starts_with("player:"))
        .find(|obj| obj.name.eq_ignore_ascii_case(nick))
        .map(|obj| obj.id.clone())
}

/// Dispatch one parsed command for a connected or connecting IRC nick.
///
/// The outer [`SessionManager`] mutex is held only for registry/login/logout work.
/// Per-nick session and world locks allow concurrent commands from different players.
pub async fn dispatch_command<P: Persistence + Clone + Send + Sync>(
    manager: Arc<Mutex<SessionManager<P>>>,
    nick: &str,
    input: &str,
    config: &IrcConfig,
) -> DispatchOutcome {
    let line = parse_command_line(input);
    let sender = normalize_nick(nick);

    let logged_in = {
        let mgr = manager.lock().await;
        mgr.session_handle(nick).is_some()
    };

    if !logged_in {
        let mut mgr = manager.lock().await;
        let persistence = mgr.persistence().clone();
        return dispatch_logged_out(&mut mgr, &persistence, nick, &line, config).await;
    }

    if line.is_meta {
        return dispatch_meta(&manager, nick, &line, sender).await;
    }

    match line.verb.as_str() {
        "help" | "?" => DispatchOutcome {
            sender,
            to_sender: vec![help_text()],
            ..Default::default()
        },
        "login" => DispatchOutcome {
            sender,
            to_sender: vec!["You are already logged in. Send 'quit' to disconnect.".to_string()],
            ..Default::default()
        },
        "quit" | "logout" | "exit" => dispatch_quit(&manager, nick, sender, config).await,
        "look" | "l" => dispatch_look(&manager, nick, &line.args, sender).await,
        "inventory" | "i" => dispatch_inventory(&manager, nick, sender).await,
        "say" | "'" => dispatch_say(&manager, nick, &line.args, sender, config).await,
        "emote" | ":" => dispatch_emote(&manager, nick, &line.args, sender, config).await,
        "tell" | "whisper" => dispatch_tell(&manager, nick, &line.args, sender).await,
        "take" | "get" => dispatch_take(&manager, nick, &line.args, sender).await,
        "go" | _ => dispatch_movement(&manager, nick, &line, sender, config).await,
    }
}

async fn dispatch_logged_out<P: Persistence + Clone>(
    manager: &mut SessionManager<P>,
    persistence: &P,
    nick: &str,
    line: &CommandLine,
    config: &IrcConfig,
) -> DispatchOutcome {
    let sender = normalize_nick(nick);
    match line.verb.as_str() {
        "login" => dispatch_login(manager, persistence, nick, &line.args, sender, config).await,
        "help" | "?" => DispatchOutcome {
            sender,
            to_sender: vec![logged_out_help_text()],
            ..Default::default()
        },
        _ => DispatchOutcome {
            sender,
            to_sender: vec!["You are not logged in. Send 'login' or 'login <player-id>'.".to_string()],
            ..Default::default()
        },
    }
}

async fn dispatch_login<P: Persistence + Clone>(
    manager: &mut SessionManager<P>,
    _persistence: &P,
    nick: &str,
    args: &[String],
    sender: String,
    config: &IrcConfig,
) -> DispatchOutcome {
    let explicit = args.first().map(String::as_str);
    let player_id = {
        let guard = manager.world().lock().await;
        resolve_player_for_login(nick, explicit, guard.objects())
    };

    let Some(player_id) = player_id else {
        return DispatchOutcome {
            sender,
            to_sender: vec![
                "No matching player found. Try 'login player:hero-001'.".to_string(),
            ],
            ..Default::default()
        };
    };

    let bootstrap_location = {
        let guard = manager.world().lock().await;
        guard.object(&player_id).and_then(|obj| obj.location.clone())
    };

    match manager.login(nick, player_id, bootstrap_location).await {
        Ok(()) => {
            let mut outcome = DispatchOutcome {
                sender: sender.clone(),
                to_sender: vec!["Welcome to MUDL. Type 'help' for commands.".to_string()],
                channel_sync: Some(ChannelSync {
                    nick: sender.clone(),
                    join: vec![
                        config.world_channel.clone(),
                        {
                            let room_id = if let Some(handle) = manager.session_handle(nick) {
                                let session = handle.lock().await;
                                session.current_location().cloned()
                            } else {
                                None
                            };
                            room_id
                                .map(|room| room_channel_name(&config.room_channel_prefix, &room))
                                .unwrap_or_default()
                        },
                    ]
                    .into_iter()
                    .filter(|c| !c.is_empty())
                    .collect(),
                    part: Vec::new(),
                }),
                persist: true,
                ..Default::default()
            };

            if let Some(handle) = manager.session_handle(nick) {
                let session = handle.lock().await;
                if let Some(room_id) = session.current_location().cloned() {
                    let ctx = session.display_context_async(DisplayMode::Player).await;
                    if let Some(room) = ctx.objects.get(&room_id) {
                        outcome
                            .to_sender
                            .push(format_room_look_player(room, &ctx));
                    }
                    let channel = room_channel_name(&config.room_channel_prefix, &room_id);
                    outcome.to_sender.push(room_join_notice(&channel));
                }
            }

            outcome
        }
        Err(err) => DispatchOutcome {
            sender,
            to_sender: vec![err.to_string()],
            ..Default::default()
        },
    }
}

async fn dispatch_quit<P: Persistence + Clone + Send + Sync>(
    manager: &Arc<Mutex<SessionManager<P>>>,
    nick: &str,
    sender: String,
    config: &IrcConfig,
) -> DispatchOutcome {
    let old_room = {
        let mgr = manager.lock().await;
        let handle = mgr.session_handle(nick);
        drop(mgr);
        if let Some(handle) = handle {
            let session = handle.lock().await;
            session.current_location().cloned()
        } else {
            None
        }
    };
    let mut mgr = manager.lock().await;
    match mgr.logout(nick).await {
        Ok(()) => {
            let mut part = vec![config.world_channel.clone()];
            if let Some(room_id) = old_room {
                part.push(room_channel_name(&config.room_channel_prefix, &room_id));
            }
            DispatchOutcome {
                sender: sender.clone(),
                to_sender: vec!["Goodbye!".to_string()],
                channel_sync: Some(ChannelSync {
                    nick: sender,
                    join: Vec::new(),
                    part,
                }),
                ..Default::default()
            }
        }
        Err(err) => DispatchOutcome {
            sender,
            to_sender: vec![err.to_string()],
            ..Default::default()
        },
    }
}

async fn dispatch_meta<P: Persistence + Clone + Send + Sync>(
    manager: &Arc<Mutex<SessionManager<P>>>,
    nick: &str,
    line: &CommandLine,
    sender: String,
) -> DispatchOutcome {
    let handle = {
        let mgr = manager.lock().await;
        mgr.session_handle(nick)
    };
    let Some(handle) = handle else {
        return DispatchOutcome {
            sender,
            to_sender: vec!["You are not logged in.".to_string()],
            ..Default::default()
        };
    };

    let session = handle.lock().await;
    let message = session
        .with_world_async(|world, player| {
            let Some(actor) = world.object(player.actor_id()) else {
                return "You seem to have lost yourself.".to_string();
            };
            let result = if line.verb == "create" || line.verb == "load" || line.verb == "save" {
                authorize_plain_command(actor, &line.verb, line.args.first().map(String::as_str))
            } else {
                authorize_meta_command(actor, &line.verb)
            };
            match result {
                Ok(()) => "Builder commands over IRC are not enabled yet. Use the REPL.".to_string(),
                Err(err) => err.to_string(),
            }
        })
        .await;

    DispatchOutcome {
        sender,
        to_sender: vec![message],
        ..Default::default()
    }
}

async fn dispatch_look<P: Persistence + Clone + Send + Sync>(
    manager: &Arc<Mutex<SessionManager<P>>>,
    nick: &str,
    args: &[String],
    sender: String,
) -> DispatchOutcome {
    let persistence = {
        let mgr = manager.lock().await;
        mgr.persistence().clone()
    };
    let handle = {
        let mgr = manager.lock().await;
        mgr.session_handle(nick)
    };
    let Some(handle) = handle else {
        return DispatchOutcome::default();
    };

    let target = args.first().map(String::as_str);
    let resolution = {
        let session = handle.lock().await;
        if let Some(name) = target {
            session.resolve_target_async(name, ResolveScope::General).await
        } else if let Some(loc) = session.current_location() {
            TargetResolution::Found(loc.clone())
        } else {
            TargetResolution::NotFound
        }
    };

    if let TargetResolution::Found(ref id) = resolution {
        let mut session = handle.lock().await;
        let _ = session.ensure_object(&persistence, id).await;
    }

    let mut lines = Vec::new();
    let mut session = handle.lock().await;
    match resolution {
        TargetResolution::Found(id) => {
            let is_room = session
                .objects_async()
                .await
                .get(&id)
                .is_some_and(|obj| obj.is_location());
            if is_room {
                let discovery = session.perceive_hidden_on_look_async().await;
                lines.extend(discovery.lines);
            }
            let ctx = session.display_context_async(DisplayMode::Player).await;
            if let Some(obj) = ctx.objects.get(&id) {
                if obj.is_location() {
                    lines.push(format_room_look_player(obj, &ctx));
                } else {
                    lines.push(obj.describe(&ctx));
                }
            } else if let Some(name) = target {
                lines.push(narrate_target_not_found(name));
            } else {
                lines.push(narrate_no_location());
            }
        }
        TargetResolution::Ambiguous(msg) => lines.push(msg),
        TargetResolution::NotFound => {
            if let Some(name) = target {
                lines.push(narrate_target_not_found(name));
            } else {
                lines.push(narrate_no_location());
            }
        }
    }

    DispatchOutcome {
        sender,
        to_sender: lines,
        ..Default::default()
    }
}

async fn dispatch_inventory<P: Persistence + Clone + Send + Sync>(
    manager: &Arc<Mutex<SessionManager<P>>>,
    nick: &str,
    sender: String,
) -> DispatchOutcome {
    let handle = {
        let mgr = manager.lock().await;
        mgr.session_handle(nick)
    };
    let Some(handle) = handle else {
        return DispatchOutcome::default();
    };

    let session = handle.lock().await;
    let text = session
        .with_world_async(|world, player| {
            world
                .object(player.actor_id())
                .map(|obj| describe_inventory(obj, world.objects(), world.anatomy()))
                .unwrap_or_else(|| "You seem to have lost yourself.".to_string())
        })
        .await;

    DispatchOutcome {
        sender,
        to_sender: vec![text],
        ..Default::default()
    }
}

async fn dispatch_say<P: Persistence + Clone + Send + Sync>(
    manager: &Arc<Mutex<SessionManager<P>>>,
    nick: &str,
    args: &[String],
    sender: String,
    config: &IrcConfig,
) -> DispatchOutcome {
    if args.is_empty() {
        return DispatchOutcome {
            sender,
            to_sender: vec!["Say what?".to_string()],
            ..Default::default()
        };
    }

    let handle = {
        let mgr = manager.lock().await;
        mgr.session_handle(nick)
    };
    let Some(handle) = handle else {
        return DispatchOutcome::default();
    };

    let room_id = {
        let session = handle.lock().await;
        session.current_location().cloned()
    };
    let Some(room_id) = room_id else {
        return DispatchOutcome {
            sender,
            to_sender: vec![narrate_no_location()],
            ..Default::default()
        };
    };

    let speaker = actor_display_name_async(&handle).await;
    let text = args.join(" ");
    let formatted = format_say(&speaker, &text);
    let mgr = manager.lock().await;
    let audience = players_in_room_async(&mgr, &room_id, Some(nick))
        .await
        .into_iter()
        .map(|p| p.nick)
        .collect();

    let room_channel = room_channel_name(&config.room_channel_prefix, &room_id);
    DispatchOutcome {
        sender,
        to_sender: vec![formatted.clone()],
        room_audience: vec![RoomDelivery {
            audience,
            lines: vec![formatted.clone()],
        }],
        channel: vec![(room_channel, formatted)],
        ..Default::default()
    }
}

async fn dispatch_emote<P: Persistence + Clone + Send + Sync>(
    manager: &Arc<Mutex<SessionManager<P>>>,
    nick: &str,
    args: &[String],
    sender: String,
    config: &IrcConfig,
) -> DispatchOutcome {
    if args.is_empty() {
        return DispatchOutcome {
            sender,
            to_sender: vec!["Emote what?".to_string()],
            ..Default::default()
        };
    }

    let handle = {
        let mgr = manager.lock().await;
        mgr.session_handle(nick)
    };
    let Some(handle) = handle else {
        return DispatchOutcome::default();
    };

    let room_id = {
        let session = handle.lock().await;
        session.current_location().cloned()
    };
    let Some(room_id) = room_id else {
        return DispatchOutcome {
            sender,
            to_sender: vec![narrate_no_location()],
            ..Default::default()
        };
    };

    let speaker = actor_display_name_async(&handle).await;
    let text = args.join(" ");
    let formatted = format_emote(&speaker, &text);
    let mgr = manager.lock().await;
    let audience = players_in_room_async(&mgr, &room_id, Some(nick))
        .await
        .into_iter()
        .map(|p| p.nick)
        .collect();
    let room_channel = room_channel_name(&config.room_channel_prefix, &room_id);

    DispatchOutcome {
        sender,
        to_sender: vec![formatted.clone()],
        room_audience: vec![RoomDelivery {
            audience,
            lines: vec![formatted.clone()],
        }],
        channel: vec![(room_channel, formatted)],
        ..Default::default()
    }
}

async fn dispatch_tell<P: Persistence + Clone + Send + Sync>(
    manager: &Arc<Mutex<SessionManager<P>>>,
    nick: &str,
    args: &[String],
    sender: String,
) -> DispatchOutcome {
    if args.len() < 2 {
        return DispatchOutcome {
            sender,
            to_sender: vec!["Usage: tell <player> <message>".to_string()],
            ..Default::default()
        };
    }

    let target_nick = &args[0];
    let text = args[1..].join(" ");
    let resolved = {
        let mgr = manager.lock().await;
        resolve_connected_nick(&mgr, target_nick)
    };
    let Some(resolved) = resolved else {
        return DispatchOutcome {
            sender,
            to_sender: vec![format!("{target_nick} is not connected.")],
            ..Default::default()
        };
    };

    if normalize_nick(&resolved) == normalize_nick(nick) {
        return DispatchOutcome {
            sender,
            to_sender: vec!["You talk to yourself.".to_string()],
            ..Default::default()
        };
    }

    let from_name = {
        let mgr = manager.lock().await;
        if let Some(handle) = mgr.session_handle(nick) {
            actor_display_name_async(&handle).await
        } else {
            nick.to_string()
        }
    };

    DispatchOutcome {
        sender,
        to_sender: vec![format_tell_sent(target_nick, &text)],
        private: vec![(resolved, format_tell(&from_name, &text))],
        ..Default::default()
    }
}

async fn dispatch_take<P: Persistence + Clone + Send + Sync>(
    manager: &Arc<Mutex<SessionManager<P>>>,
    nick: &str,
    args: &[String],
    sender: String,
) -> DispatchOutcome {
    let Some(target) = args.first() else {
        return DispatchOutcome {
            sender,
            to_sender: vec!["Take what?".to_string()],
            ..Default::default()
        };
    };

    let handle = {
        let mgr = manager.lock().await;
        mgr.session_handle(nick)
    };
    let Some(handle) = handle else {
        return DispatchOutcome::default();
    };

    let mut session = handle.lock().await;
    let result = session.with_inventory_async(|ctx| take_item(ctx, target)).await;
    match result {
        Ok(msg) => DispatchOutcome {
            sender,
            to_sender: vec![msg],
            persist: true,
            ..Default::default()
        },
        Err(InventoryError::NotFound(_)) => DispatchOutcome {
            sender,
            to_sender: vec![narrate_target_not_found(target)],
            ..Default::default()
        },
        Err(err) => DispatchOutcome {
            sender,
            to_sender: vec![err.to_string()],
            ..Default::default()
        },
    }
}

async fn dispatch_movement<P: Persistence + Clone + Send + Sync>(
    manager: &Arc<Mutex<SessionManager<P>>>,
    nick: &str,
    line: &CommandLine,
    sender: String,
    config: &IrcConfig,
) -> DispatchOutcome {
    let handle = {
        let mgr = manager.lock().await;
        mgr.session_handle(nick)
    };
    let Some(handle) = handle else {
        return DispatchOutcome::default();
    };

    let index = {
        let session = handle.lock().await;
        session
            .with_world_async(|world, player| {
                player
                    .current_location()
                    .and_then(|loc| world.object(loc))
                    .map(exit_index)
            })
            .await
    };

    let arg_refs: Vec<&str> = line.args.iter().map(String::as_str).collect();
    let direction = movement_from_line(&line.verb, &arg_refs, index.as_ref());
    let Some(direction) = direction else {
        return DispatchOutcome {
            sender,
            to_sender: vec![format!("Unknown command: {}. Type 'help'.", line.verb)],
            ..Default::default()
        };
    };

    let old_room = {
        let session = handle.lock().await;
        session.current_location().cloned()
    };
    let mut session = handle.lock().await;
    match session.go_async(&direction).await {
        Ok(msg) => {
            let mut outcome = DispatchOutcome {
                sender,
                to_sender: msg.lines().map(str::to_string).collect(),
                persist: true,
                ..Default::default()
            };

            if let (Some(old_id), Some(new_id)) = (old_room, session.current_location()) {
                if old_id != *new_id {
                    outcome.channel_sync = Some(ChannelSync {
                        nick: nick.to_string(),
                        join: vec![room_channel_name(&config.room_channel_prefix, new_id)],
                        part: vec![room_channel_name(&config.room_channel_prefix, &old_id)],
                    });
                    outcome.to_sender.push(room_join_notice(&room_channel_name(
                        &config.room_channel_prefix,
                        new_id,
                    )));
                }
            }

            outcome
        }
        Err(err) => DispatchOutcome {
            sender,
            to_sender: vec![err.to_string()],
            ..Default::default()
        },
    }
}

fn help_text() -> String {
    [
        "MUDL IRC commands:",
        "  look (l) [target]   - view room or object",
        "  go <dir>            - move (or use exit name: north, n, ...)",
        "  inventory (i)       - list carried items",
        "  take <item>         - pick up an item",
        "  say <text>          - speak to players in your room",
        "  emote <text>        - perform an action in your room",
        "  tell <nick> <text>  - private message to a connected player",
        "  quit                - save and disconnect",
        "World channel (#mudl): prefix with 'say' is not used — speak freely for OOC.",
    ]
    .join("\n")
}

fn logged_out_help_text() -> String {
    "Send 'login' to bind your IRC nick to a player, or 'login <player-id>'."
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::SessionManager;
    use crate::irc::config::IrcConfig;
    use crate::object::{Object, PermissionFlags};
    use crate::persistence::SqlitePersistence;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::Mutex;

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

    async fn sample_manager() -> (SqlitePersistence, SessionManager<SqlitePersistence>, IrcConfig) {
        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let room = ObjectId::new("room:void-001");
        let north = ObjectId::new("room:north-001");

        let mut hero1 = bare("player:hero-001", "Alice");
        hero1.location = Some(room.clone());
        let mut hero2 = bare("player:hero-002", "Bob");
        hero2.location = Some(room.clone());

        let mut place = bare("room:void-001", "The Void");
        place.set_property_map(
            "exits",
            HashMap::from([("north".to_string(), north.clone())]),
        );
        let mut north_room = bare("room:north-001", "North");
        north_room.add_exit("south", room.clone());

        persistence.save_object(&hero1).await.unwrap();
        persistence.save_object(&hero2).await.unwrap();
        persistence.save_object(&place).await.unwrap();
        persistence.save_object(&north_room).await.unwrap();

        let manager = SessionManager::open(persistence.clone(), crate::mudl::AnatomyRegistry::default())
            .await
            .unwrap();
        (persistence, manager, IrcConfig::default())
    }

    async fn manager_arc() -> (Arc<Mutex<SessionManager<SqlitePersistence>>>, IrcConfig) {
        let (_persistence, manager, config) = sample_manager().await;
        (Arc::new(Mutex::new(manager)), config)
    }

    #[tokio::test]
    async fn login_binds_nick_to_player_name() {
        let (manager, config) = manager_arc().await;
        let outcome = dispatch_command(Arc::clone(&manager), "alice", "login", &config).await;
        assert!(outcome.to_sender.iter().any(|l| l.contains("Welcome")));
        assert!(manager.lock().await.is_connected("alice"));
    }

    #[tokio::test]
    async fn say_reaches_co_located_player() {
        let (manager, config) = manager_arc().await;
        dispatch_command(Arc::clone(&manager), "alice", "login", &config).await;
        dispatch_command(Arc::clone(&manager), "bob", "login", &config).await;

        let outcome =
            dispatch_command(manager, "alice", "say hello there", &config).await;
        assert_eq!(outcome.room_audience.len(), 1);
        assert_eq!(outcome.room_audience[0].audience, vec!["bob".to_string()]);
        assert!(outcome.room_audience[0].lines[0].contains("hello there"));
        assert!(outcome.channel[0].0.contains("void-001"));
    }

    #[tokio::test]
    async fn tell_is_private_between_players() {
        let (manager, config) = manager_arc().await;
        dispatch_command(Arc::clone(&manager), "alice", "login", &config).await;
        dispatch_command(Arc::clone(&manager), "bob", "login", &config).await;

        let outcome =
            dispatch_command(manager, "alice", "tell bob secret", &config).await;
        assert_eq!(outcome.private.len(), 1);
        assert_eq!(outcome.private[0].0, "bob");
        assert!(outcome.private[0].1.contains("secret"));
    }

    #[tokio::test]
    async fn movement_syncs_room_channels() {
        let (manager, config) = manager_arc().await;
        dispatch_command(Arc::clone(&manager), "alice", "login", &config).await;

        let outcome = dispatch_command(manager, "alice", "go north", &config).await;
        let sync = outcome.channel_sync.expect("channel sync");
        assert!(sync.join.iter().any(|c| c.contains("north-001")));
        assert!(sync.part.iter().any(|c| c.contains("void-001")));
    }
}