//! IRC transport adapter: rate limits, login lifecycle, and [`CommandResult`] delivery.
//!
//! Player verbs route through [`CommandDispatcher`](crate::command::CommandDispatcher);
//! this module maps transport-neutral results to [`DispatchOutcome`].

use std::sync::Arc;

use tokio::sync::Mutex;

use crate::command::parse_command_line;
use crate::command::{
    CommandDispatcher, CommandLine, CommandResult, LookOptions, PlayerDispatchOptions, SocialIntent,
};
use crate::display::{format_room_look_player, DisplayMode};
use crate::gateway::{
    normalize_nick, parse_login_args, rate_limit_kind_for_line, resolve_player_for_login,
    verify_login, LoginRequest, RateLimitKind, SessionManager,
};
use crate::persistence::Persistence;

use super::channels::{room_channel_name, room_join_notice};
use super::config::IrcConfig;
use super::social::{format_emote, format_say, format_tell, format_tell_sent};
use super::visibility::{
    irc_look_scope, players_in_room_async, resolve_connected_nick,
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

    if line.verb.is_empty() {
        let hint = if logged_in {
            "Send 'help' for a list of commands."
        } else {
            "Send 'login' to connect."
        };
        return DispatchOutcome {
            sender,
            to_sender: vec![hint.to_string()],
            ..Default::default()
        };
    }

    let rate_kind = rate_limit_kind_for_line(&line);
    if rate_kind != RateLimitKind::Movement {
        let mgr = manager.lock().await;
        if let Err(denied) = mgr.check_rate_limit(nick, rate_kind) {
            return DispatchOutcome {
                sender,
                to_sender: vec![mgr.rate_limit_denial_message(denied.kind)],
                ..Default::default()
            };
        }
    }

    if !logged_in {
        let mut mgr = manager.lock().await;
        let persistence = mgr.persistence().clone();
        return dispatch_logged_out(&mut mgr, &persistence, nick, &line, config).await;
    }

    if matches!(line.verb.as_str(), "quit" | "logout" | "exit") {
        return dispatch_quit(&manager, nick, sender, config).await;
    }

    dispatch_player_command(&manager, nick, &line, sender, config).await
}

async fn dispatch_player_command<P: Persistence + Clone + Send + Sync>(
    manager: &Arc<Mutex<SessionManager<P>>>,
    nick: &str,
    line: &CommandLine,
    sender: String,
    config: &IrcConfig,
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
        return DispatchOutcome {
            sender,
            to_sender: vec!["You are not logged in.".to_string()],
            ..Default::default()
        };
    };

    let options = PlayerDispatchOptions {
        look: LookOptions::player(irc_look_scope()),
    };
    let result = {
        let mut session = handle.lock().await;
        CommandDispatcher::dispatch_player_line(&mut session, &persistence, line, &options).await
    };

    deliver_command_result(result, sender, nick, manager, config).await
}

async fn deliver_command_result<P: Persistence + Clone + Send + Sync>(
    result: CommandResult,
    sender: String,
    nick: &str,
    manager: &Arc<Mutex<SessionManager<P>>>,
    config: &IrcConfig,
) -> DispatchOutcome {
    let mut outcome = DispatchOutcome {
        sender,
        to_sender: result.lines_to_actor,
        persist: result.persist_world,
        ..Default::default()
    };

    if let Some(social) = result.social {
        match social {
            SocialIntent::Say {
                room_id,
                speaker_name,
                text,
            } => {
                let formatted = format_say(&speaker_name, &text);
                outcome.to_sender.push(formatted.clone());
                let mgr = manager.lock().await;
                let audience = players_in_room_async(&mgr, &room_id, Some(nick))
                    .await
                    .into_iter()
                    .map(|p| p.nick)
                    .collect();
                let room_channel = room_channel_name(&config.room_channel_prefix, &room_id);
                outcome.room_audience.push(RoomDelivery {
                    audience,
                    lines: vec![formatted.clone()],
                });
                outcome.channel.push((room_channel, formatted));
            }
            SocialIntent::Emote {
                room_id,
                speaker_name,
                text,
            } => {
                let formatted = format_emote(&speaker_name, &text);
                outcome.to_sender.push(formatted.clone());
                let mgr = manager.lock().await;
                let audience = players_in_room_async(&mgr, &room_id, Some(nick))
                    .await
                    .into_iter()
                    .map(|p| p.nick)
                    .collect();
                let room_channel = room_channel_name(&config.room_channel_prefix, &room_id);
                outcome.room_audience.push(RoomDelivery {
                    audience,
                    lines: vec![formatted.clone()],
                });
                outcome.channel.push((room_channel, formatted));
            }
            SocialIntent::Tell {
                target_identity,
                speaker_name,
                text,
            } => {
                let mgr = manager.lock().await;
                let Some(resolved) = resolve_connected_nick(&mgr, &target_identity) else {
                    outcome.to_sender = vec![format!("{target_identity} is not connected.")];
                    outcome.persist = false;
                    return outcome;
                };
                if normalize_nick(&resolved) == normalize_nick(nick) {
                    outcome.to_sender = vec!["You talk to yourself.".to_string()];
                    outcome.persist = false;
                    return outcome;
                }
                outcome.to_sender = vec![format_tell_sent(&resolved, &text)];
                outcome
                    .private
                    .push((resolved, format_tell(&speaker_name, &text)));
            }
        }
    }

    if let Some(movement) = result.movement {
        outcome.to_sender.extend(movement.lines);
        if let (Some(old_id), Some(new_id)) = (movement.old_room, movement.new_room) {
            if old_id != new_id {
                outcome.channel_sync = Some(ChannelSync {
                    nick: outcome.sender.clone(),
                    join: vec![room_channel_name(&config.room_channel_prefix, &new_id)],
                    part: vec![room_channel_name(&config.room_channel_prefix, &old_id)],
                });
                outcome.to_sender.push(room_join_notice(&room_channel_name(
                    &config.room_channel_prefix,
                    &new_id,
                )));
            }
        }
    }

    outcome
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
            to_sender: vec![logged_out_help_text(config)],
            ..Default::default()
        },
        _ => DispatchOutcome {
            sender,
            to_sender: vec![format!(
                "You are not logged in. {}",
                config.login_auth.logged_out_help()
            )],
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
    let parsed = parse_login_args(args);
    let (player_id, player_snapshot, bootstrap_location) = {
        let guard = manager.world().lock().await;
        let player_id = resolve_player_for_login(
            nick,
            &parsed,
            &config.login_auth,
            guard.objects(),
        );
        let Some(player_id) = player_id else {
            return DispatchOutcome {
                sender,
                to_sender: vec!["Invalid login credentials.".to_string()],
                ..Default::default()
            };
        };
        let player_snapshot = guard.object(&player_id).cloned();
        let bootstrap_location = player_snapshot
            .as_ref()
            .and_then(|obj| obj.location.clone());
        (player_id, player_snapshot, bootstrap_location)
    };

    let Some(player) = player_snapshot else {
        return DispatchOutcome {
            sender,
            to_sender: vec!["Invalid login credentials.".to_string()],
            ..Default::default()
        };
    };

    if let Err(err) = verify_login(
        &config.login_auth,
        LoginRequest {
            transport: "irc",
            identity: nick,
            player_id: &player_id,
            token: parsed.token.as_deref(),
            player: &player,
        },
    ) {
        return DispatchOutcome {
            sender,
            to_sender: vec![err.to_string()],
            ..Default::default()
        };
    }

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

fn logged_out_help_text(config: &IrcConfig) -> String {
    config.login_auth.logged_out_help()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::SessionManager;
    use crate::irc::config::IrcConfig;
    use crate::object::{Object, ObjectId, PermissionFlags};
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

    async fn manager_arc_with_rate_limits(
        rate_config: crate::gateway::RateLimitConfig,
    ) -> (Arc<Mutex<SessionManager<SqlitePersistence>>>, IrcConfig) {
        let (persistence, _manager, mut config) = sample_manager().await;
        let manager = SessionManager::from_world_with_rate_limits(
            persistence,
            _manager.world().clone(),
            rate_config.clone(),
        );
        config.rate_limits = rate_config;
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
    async fn empty_command_prompts_for_login_when_logged_out() {
        let (manager, config) = manager_arc().await;
        let outcome = dispatch_command(manager, "alice", "   ", &config).await;
        assert!(outcome.to_sender.iter().any(|l| l.contains("login")));
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

    #[tokio::test]
    async fn login_denied_without_token_when_auth_required() {
        let (manager, mut config) = manager_arc().await;
        config.login_auth = crate::gateway::LoginAuthPolicy {
            require_auth: true,
            env_tokens: HashMap::from([(
                "player:hero-001".to_string(),
                "alice-secret".to_string(),
            )]),
            ..crate::gateway::LoginAuthPolicy::permissive()
        };

        let outcome = dispatch_command(manager, "alice", "login", &config).await;
        assert!(outcome
            .to_sender
            .iter()
            .any(|l| l.contains("Invalid login credentials")));
    }

    #[tokio::test]
    async fn login_succeeds_with_token_when_auth_required() {
        let (manager, mut config) = manager_arc().await;
        config.login_auth = crate::gateway::LoginAuthPolicy {
            require_auth: true,
            env_tokens: HashMap::from([(
                "player:hero-001".to_string(),
                "alice-secret".to_string(),
            )]),
            ..crate::gateway::LoginAuthPolicy::permissive()
        };

        let outcome = dispatch_command(
            manager.clone(),
            "alice",
            "login player:hero-001 alice-secret",
            &config,
        )
        .await;
        assert!(outcome.to_sender.iter().any(|l| l.contains("Welcome")));
        assert!(manager.lock().await.is_connected("alice"));
    }

    #[tokio::test]
    async fn login_token_only_resolves_player() {
        let (manager, mut config) = manager_arc().await;
        config.login_auth = crate::gateway::LoginAuthPolicy {
            require_auth: true,
            env_tokens: HashMap::from([(
                "player:hero-001".to_string(),
                "tok-only".to_string(),
            )]),
            ..crate::gateway::LoginAuthPolicy::permissive()
        };

        let outcome =
            dispatch_command(manager.clone(), "any-nick", "login tok-only", &config).await;
        assert!(outcome.to_sender.iter().any(|l| l.contains("Welcome")));
        assert!(manager.lock().await.is_connected("any-nick"));
    }

    async fn manager_with_sword_at(
        room_id: &ObjectId,
    ) -> (Arc<Mutex<SessionManager<SqlitePersistence>>>, IrcConfig) {
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

        let mut sword = bare("item:rusty-sword", "rusty sword");
        sword.location = Some(room_id.clone());

        persistence.save_object(&hero1).await.unwrap();
        persistence.save_object(&hero2).await.unwrap();
        persistence.save_object(&place).await.unwrap();
        persistence.save_object(&north_room).await.unwrap();
        persistence.save_object(&sword).await.unwrap();

        let manager = SessionManager::open(persistence, crate::mudl::AnatomyRegistry::default())
            .await
            .unwrap();
        (Arc::new(Mutex::new(manager)), IrcConfig::default())
    }

    #[tokio::test]
    async fn look_resolves_targets_in_current_room() {
        let room = ObjectId::new("room:void-001");
        let (manager, config) = manager_with_sword_at(&room).await;
        dispatch_command(Arc::clone(&manager), "alice", "login", &config).await;

        let outcome = dispatch_command(manager, "alice", "look rusty sword", &config).await;
        assert!(
            outcome
                .to_sender
                .iter()
                .any(|l| l.contains("rusty sword") && !l.contains("don't see")),
            "expected in-room target description, got: {:?}",
            outcome.to_sender
        );
    }

    #[tokio::test]
    async fn look_rejects_cross_room_targets_by_name() {
        let north = ObjectId::new("room:north-001");
        let (manager, config) = manager_with_sword_at(&north).await;
        dispatch_command(Arc::clone(&manager), "alice", "login", &config).await;

        let outcome = dispatch_command(manager, "alice", "look rusty sword", &config).await;
        assert!(outcome
            .to_sender
            .iter()
            .any(|l| l.contains("don't see anything like \"rusty sword\"")));
    }

    #[tokio::test]
    async fn look_rejects_cross_room_player_by_id() {
        let (manager, config) = manager_arc().await;
        dispatch_command(Arc::clone(&manager), "alice", "login", &config).await;
        dispatch_command(Arc::clone(&manager), "bob", "login", &config).await;
        dispatch_command(Arc::clone(&manager), "bob", "go north", &config).await;

        let outcome =
            dispatch_command(manager, "alice", "look player:hero-002", &config).await;
        assert!(outcome
            .to_sender
            .iter()
            .any(|l| l.contains("don't see anything like \"player:hero-002\"")));
    }

    #[tokio::test]
    async fn look_finds_co_located_player() {
        let (manager, config) = manager_arc().await;
        dispatch_command(Arc::clone(&manager), "alice", "login", &config).await;
        dispatch_command(Arc::clone(&manager), "bob", "login", &config).await;

        let outcome = dispatch_command(manager, "alice", "look bob", &config).await;
        assert!(
            outcome.to_sender.iter().any(|l| l.contains("Bob")),
            "expected co-located player description, got: {:?}",
            outcome.to_sender
        );
    }

    #[tokio::test]
    async fn command_flood_is_rate_limited() {
        let rate_config = crate::gateway::RateLimitConfig {
            enabled: true,
            commands: crate::gateway::BucketSpec::new(2, 60.0),
            movement: crate::gateway::BucketSpec::new(10, 10.0),
            ooc: crate::gateway::BucketSpec::new(10, 30.0),
        };
        let (manager, config) = manager_arc_with_rate_limits(rate_config).await;
        dispatch_command(Arc::clone(&manager), "alice", "login", &config).await;

        dispatch_command(Arc::clone(&manager), "alice", "look", &config).await;
        dispatch_command(Arc::clone(&manager), "alice", "look", &config).await;
        let denied = dispatch_command(manager, "alice", "look", &config).await;
        assert!(denied
            .to_sender
            .iter()
            .any(|l| l.contains("too quickly")));
    }

    #[tokio::test]
    async fn movement_flood_is_rate_limited() {
        let rate_config = crate::gateway::RateLimitConfig {
            enabled: true,
            commands: crate::gateway::BucketSpec::new(30, 60.0),
            movement: crate::gateway::BucketSpec::new(1, 10.0),
            ooc: crate::gateway::BucketSpec::new(10, 30.0),
        };
        let (manager, config) = manager_arc_with_rate_limits(rate_config).await;
        dispatch_command(Arc::clone(&manager), "alice", "login", &config).await;

        dispatch_command(Arc::clone(&manager), "alice", "go north", &config).await;
        let denied = dispatch_command(manager, "alice", "go south", &config).await;
        assert!(denied
            .to_sender
            .iter()
            .any(|l| l.contains("moving too quickly")));
    }
}