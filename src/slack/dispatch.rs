//! Slack transport adapter: rate limits, login lifecycle, and [`CommandResult`] delivery.
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
    actor_place_context, format_open_context_post, open_channel_broadcast_body, parse_login_args,
    rate_limit_kind_for_line, resolve_player_for_login, verify_login, LoginRequest, RateLimitKind,
    SessionManager,
};
use crate::irc::{format_emote, format_say, format_tell, format_tell_sent};
use crate::persistence::Persistence;

use super::channels::{ic_join_notice, login_presence_joins, logout_presence_parts, speech_presence};
use super::config::SlackConfig;
use crate::irc::connected_speech_audience_async;

use super::multi_user::append_movement_visibility;
use super::session::slack_logged_out_help;
use super::visibility::{resolve_connected_user_async, slack_look_scope};

/// Slack routing instructions produced by command dispatch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DispatchOutcome {
    /// Slack user id (`U…`) — transport registry identity.
    pub user_id: String,
    /// DM conversation id (`D…`) where actor-facing lines are delivered.
    pub reply_channel: String,
    pub to_sender: Vec<String>,
    /// Private tells: `(target_user_id, line)`.
    pub private: Vec<(String, String)>,
    pub room_audience: Vec<RoomDelivery>,
    /// Shared presence posts: `(presence_key, line)` — channel slug or `C:thread:TS`.
    pub channel: Vec<(String, String)>,
    pub presence_sync: Option<PresenceSync>,
    pub persist: bool,
}

/// Lines delivered to co-located players (excluding the speaker).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomDelivery {
    pub audience: Vec<String>,
    pub lines: Vec<String>,
}

/// Join/leave instructions for room channels or threads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresenceSync {
    pub user_id: String,
    pub join: Vec<String>,
    pub part: Vec<String>,
}

/// Dispatch one command from a Slack user DM.
///
/// `user_id` is the workspace member id; `reply_channel` is the DM conversation id
/// where responses should be posted.
pub async fn dispatch_command<P: Persistence + Clone + Send + Sync>(
    manager: Arc<Mutex<SessionManager<P>>>,
    user_id: &str,
    reply_channel: &str,
    input: &str,
    config: &SlackConfig,
) -> DispatchOutcome {
    let line = parse_command_line(input);
    let user_id = user_id.trim().to_string();
    let reply_channel = reply_channel.trim().to_string();

    let logged_in = {
        let mgr = manager.lock().await;
        mgr.session_handle(&user_id).is_some()
    };

    if line.verb.is_empty() {
        let hint = if logged_in {
            "Send 'help' for a list of commands."
        } else {
            "Send 'login' to connect."
        };
        return DispatchOutcome {
            user_id,
            reply_channel,
            to_sender: vec![hint.to_string()],
            ..Default::default()
        };
    }

    let rate_kind = rate_limit_kind_for_line(&line);
    if rate_kind != RateLimitKind::Movement {
        let mgr = manager.lock().await;
        if let Err(denied) = mgr.check_rate_limit(&user_id, rate_kind) {
            return DispatchOutcome {
                user_id,
                reply_channel,
                to_sender: vec![mgr.rate_limit_denial_message(denied.kind)],
                ..Default::default()
            };
        }
    }

    if !logged_in {
        let mut mgr = manager.lock().await;
        let persistence = mgr.persistence().clone();
        return dispatch_logged_out(
            &mut mgr,
            &persistence,
            &user_id,
            &reply_channel,
            &line,
            config,
        )
        .await;
    }

    if matches!(line.verb.as_str(), "help" | "?") {
        return DispatchOutcome {
            user_id,
            reply_channel,
            to_sender: slack_help_lines(config),
            ..Default::default()
        };
    }

    if matches!(line.verb.as_str(), "quit" | "logout" | "exit") {
        return dispatch_quit(&manager, &user_id, &reply_channel, config).await;
    }

    dispatch_player_command(&manager, &user_id, &reply_channel, &line, config).await
}

async fn dispatch_player_command<P: Persistence + Clone + Send + Sync>(
    manager: &Arc<Mutex<SessionManager<P>>>,
    user_id: &str,
    reply_channel: &str,
    line: &CommandLine,
    config: &SlackConfig,
) -> DispatchOutcome {
    let persistence = {
        let mgr = manager.lock().await;
        mgr.persistence().clone()
    };
    let handle = {
        let mgr = manager.lock().await;
        mgr.session_handle(user_id)
    };
    let Some(handle) = handle else {
        return DispatchOutcome {
            user_id: user_id.to_string(),
            reply_channel: reply_channel.to_string(),
            to_sender: vec!["You are not logged in.".to_string()],
            ..Default::default()
        };
    };

    let options = PlayerDispatchOptions {
        look: LookOptions::player(slack_look_scope(config.play_mode)),
    };
    let result = {
        let mut session = handle.lock().await;
        CommandDispatcher::dispatch_player_line(&mut session, &persistence, line, &options).await
    };

    deliver_command_result(
        result,
        user_id.to_string(),
        reply_channel.to_string(),
        user_id,
        manager,
        config,
    )
    .await
}

async fn deliver_command_result<P: Persistence + Clone + Send + Sync>(
    result: CommandResult,
    user_id: String,
    reply_channel: String,
    actor_id: &str,
    manager: &Arc<Mutex<SessionManager<P>>>,
    config: &SlackConfig,
) -> DispatchOutcome {
    let social_for_broadcast = result.social.clone();
    let mut outcome = DispatchOutcome {
        user_id: user_id.clone(),
        reply_channel,
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
                let presence = speech_presence(config, &room_id);
                if config.play_mode.is_story() {
                    let mgr = manager.lock().await;
                    let audience = connected_speech_audience_async(
                        &mgr,
                        &room_id,
                        Some(actor_id),
                        config.play_mode,
                    )
                    .await;
                    outcome.room_audience.push(RoomDelivery {
                        audience,
                        lines: vec![formatted.clone()],
                    });
                }
                outcome.channel.push((presence, formatted));
            }
            SocialIntent::Emote {
                room_id,
                speaker_name,
                text,
            } => {
                let formatted = format_emote(&speaker_name, &text);
                outcome.to_sender.push(formatted.clone());
                let presence = speech_presence(config, &room_id);
                if config.play_mode.is_story() {
                    let mgr = manager.lock().await;
                    let audience = connected_speech_audience_async(
                        &mgr,
                        &room_id,
                        Some(actor_id),
                        config.play_mode,
                    )
                    .await;
                    outcome.room_audience.push(RoomDelivery {
                        audience,
                        lines: vec![formatted.clone()],
                    });
                }
                outcome.channel.push((presence, formatted));
            }
            SocialIntent::Tell {
                target_identity,
                speaker_name,
                text,
            } => {
                let mgr = manager.lock().await;
                let Some(resolved) =
                    resolve_connected_user_async(&mgr, &target_identity).await
                else {
                    outcome.to_sender =
                        vec![format!("{target_identity} is not connected.")];
                    outcome.persist = false;
                    return outcome;
                };
                if crate::gateway::normalize_nick(&resolved)
                    == crate::gateway::normalize_nick(actor_id)
                {
                    outcome.to_sender = vec!["You talk to yourself.".to_string()];
                    outcome.persist = false;
                    return outcome;
                }
                outcome.to_sender = vec![format_tell_sent(&target_identity, &text)];
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
                if config.play_mode.is_story() {
                    outcome.presence_sync = Some(PresenceSync {
                        user_id: outcome.user_id.clone(),
                        join: vec![speech_presence(config, &new_id)],
                        part: vec![speech_presence(config, &old_id)],
                    });
                    outcome
                        .to_sender
                        .push(ic_join_notice(config, &new_id));
                }

                let mgr = manager.lock().await;
                append_movement_visibility(
                    &mut outcome,
                    &mgr,
                    actor_id,
                    &old_id,
                    &new_id,
                    config,
                )
                .await;
            }
        }
    }

    if config.play_mode.is_open() {
        append_open_channel_broadcast(
            &mut outcome,
            social_for_broadcast.as_ref(),
            manager,
            actor_id,
            config,
        )
        .await;
    }

    outcome
}

async fn append_open_channel_broadcast<P: Persistence + Clone + Send + Sync>(
    outcome: &mut DispatchOutcome,
    social: Option<&SocialIntent>,
    manager: &Arc<Mutex<SessionManager<P>>>,
    actor_id: &str,
    config: &SlackConfig,
) {
    let Some(body) = open_channel_broadcast_body(social, &outcome.to_sender) else {
        return;
    };
    let mgr = manager.lock().await;
    let Some((speaker, room_id, room_name)) = actor_place_context(&mgr, actor_id).await else {
        return;
    };
    let formatted = format_open_context_post(&speaker, &room_name, &body);
    outcome
        .channel
        .push((speech_presence(config, &room_id), formatted));
}

async fn dispatch_logged_out<P: Persistence + Clone>(
    manager: &mut SessionManager<P>,
    persistence: &P,
    user_id: &str,
    reply_channel: &str,
    line: &CommandLine,
    config: &SlackConfig,
) -> DispatchOutcome {
    match line.verb.as_str() {
        "login" => {
            dispatch_login(manager, persistence, user_id, reply_channel, &line.args, config)
                .await
        }
        "help" | "?" => {
            let mut lines = vec![logged_out_help_text(config)];
            lines.extend(slack_help_lines(config));
            DispatchOutcome {
                user_id: user_id.to_string(),
                reply_channel: reply_channel.to_string(),
                to_sender: lines,
                ..Default::default()
            }
        }
        _ => DispatchOutcome {
            user_id: user_id.to_string(),
            reply_channel: reply_channel.to_string(),
            to_sender: vec![format!(
                "You are not logged in. {}",
                slack_logged_out_help(&config.login_auth)
            )],
            ..Default::default()
        },
    }
}

async fn dispatch_login<P: Persistence + Clone>(
    manager: &mut SessionManager<P>,
    _persistence: &P,
    user_id: &str,
    reply_channel: &str,
    args: &[String],
    config: &SlackConfig,
) -> DispatchOutcome {
    let parsed = parse_login_args(args);
    let (player_id, player_snapshot, bootstrap_location) = {
        let guard = manager.world().lock().await;
        let player_id = resolve_player_for_login(
            user_id,
            &parsed,
            &config.login_auth,
            guard.objects(),
        );
        let Some(player_id) = player_id else {
            return DispatchOutcome {
                user_id: user_id.to_string(),
                reply_channel: reply_channel.to_string(),
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
            user_id: user_id.to_string(),
            reply_channel: reply_channel.to_string(),
            to_sender: vec!["Invalid login credentials.".to_string()],
            ..Default::default()
        };
    };

    if let Err(err) = verify_login(
        &config.login_auth,
        LoginRequest {
            transport: "slack",
            identity: user_id,
            player_id: &player_id,
            token: parsed.token.as_deref(),
            player: &player,
        },
    ) {
        return DispatchOutcome {
            user_id: user_id.to_string(),
            reply_channel: reply_channel.to_string(),
            to_sender: vec![err.to_string()],
            ..Default::default()
        };
    }

    match manager.login(user_id, player_id, bootstrap_location).await {
        Ok(()) => {
            let room_id = if let Some(handle) = manager.session_handle(user_id) {
                let session = handle.lock().await;
                session.current_location().cloned()
            } else {
                None
            };
            let join = login_presence_joins(config, room_id.as_ref());

            let mut outcome = DispatchOutcome {
                user_id: user_id.to_string(),
                reply_channel: reply_channel.to_string(),
                to_sender: vec!["Welcome to MUDL. Type 'help' for commands.".to_string()],
                presence_sync: Some(PresenceSync {
                    user_id: user_id.to_string(),
                    join,
                    part: Vec::new(),
                }),
                persist: true,
                ..Default::default()
            };

            if let Some(handle) = manager.session_handle(user_id) {
                let session = handle.lock().await;
                if let Some(room_id) = session.current_location().cloned() {
                    let ctx = session.display_context_async(DisplayMode::Player).await;
                    if let Some(room) = ctx.objects.get(&room_id) {
                        outcome
                            .to_sender
                            .push(format_room_look_player(room, &ctx));
                    }
                    outcome
                        .to_sender
                        .push(ic_join_notice(config, &room_id));
                }
            }

            outcome
        }
        Err(err) => DispatchOutcome {
            user_id: user_id.to_string(),
            reply_channel: reply_channel.to_string(),
            to_sender: vec![err.to_string()],
            ..Default::default()
        },
    }
}

async fn dispatch_quit<P: Persistence + Clone + Send + Sync>(
    manager: &Arc<Mutex<SessionManager<P>>>,
    user_id: &str,
    reply_channel: &str,
    config: &SlackConfig,
) -> DispatchOutcome {
    let old_room = {
        let mgr = manager.lock().await;
        let handle = mgr.session_handle(user_id);
        drop(mgr);
        if let Some(handle) = handle {
            let session = handle.lock().await;
            session.current_location().cloned()
        } else {
            None
        }
    };
    let mut mgr = manager.lock().await;
    match mgr.logout(user_id).await {
        Ok(()) => {
            let part = logout_presence_parts(config, old_room.as_ref());
            DispatchOutcome {
                user_id: user_id.to_string(),
                reply_channel: reply_channel.to_string(),
                to_sender: vec!["Goodbye!".to_string()],
                presence_sync: Some(PresenceSync {
                    user_id: user_id.to_string(),
                    join: Vec::new(),
                    part,
                }),
                ..Default::default()
            }
        }
        Err(err) => DispatchOutcome {
            user_id: user_id.to_string(),
            reply_channel: reply_channel.to_string(),
            to_sender: vec![err.to_string()],
            ..Default::default()
        },
    }
}

fn logged_out_help_text(config: &SlackConfig) -> String {
    slack_logged_out_help(&config.login_auth)
}

fn slack_help_lines(config: &SlackConfig) -> Vec<String> {
    let ooc = if config.play_mode.is_open() {
        let shared = super::channels::shared_ic_presence(config);
        format!(
            "Open world: chat and commands in <#{shared}> (plain chat, no [OOC] prefix)."
        )
    } else if config.world_channel.is_empty() {
        "OOC: configured world channel (ask your operator).".to_string()
    } else {
        format!(
            "OOC: post in the world channel (<#{}>) without a command prefix.",
            config.world_channel
        )
    };
    let rooms = if config.play_mode.is_open() {
        let shared = super::channels::shared_ic_presence(config);
        format!("Look/go/say/emote from DM or <#{shared}> — output appears in-channel.")
    } else if config.rooms_channel.is_some() {
        format!(
            "In-character speech posts as threads in <#{}>.",
            config.rooms_channel.as_deref().unwrap_or("")
        )
    } else {
        "In-character speech posts to per-room channels (e.g. mudl-void-001).".to_string()
    };
    vec![
        "MUDL Slack commands (send in a DM to this bot):".to_string(),
        "  look (l) [target]   - view room or object".to_string(),
        "  go <dir>            - move (or use exit name: north, n, ...)".to_string(),
        "  inventory (i)       - list carried items".to_string(),
        "  take <item>         - pick up an item".to_string(),
        "  drop [count] <item> - drop a carried item".to_string(),
        "  attack <creature>   - strike a creature".to_string(),
        "  say <text>          - speak to players in your room".to_string(),
        "  emote <text>        - perform an action in your room".to_string(),
        "  tell <name> <text>  - private message (player name or user id)".to_string(),
        "  quit                - save and disconnect".to_string(),
        ooc,
        rooms,
    ]
}

/// Help text override wired through [`CommandDispatcher`] meta path — exported for tests.
pub fn slack_help_text(config: &SlackConfig) -> String {
    slack_help_lines(config).join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{Object, ObjectId, PermissionFlags};
    use crate::persistence::SqlitePersistence;
    use std::collections::HashMap;

    fn human_anatomy() -> crate::mudl::AnatomyRegistry {
        use crate::mudl::{BodySlotDef, CreatureDef, SlotType};
        let mut anatomy = crate::mudl::AnatomyRegistry::default();
        anatomy.creatures.insert(
            "human".to_string(),
            CreatureDef {
                name: "human".to_string(),
                slots: vec![
                    BodySlotDef {
                        name: "left_hand".to_string(),
                        capacity: 1,
                        slot_type: SlotType::Grasp,
                        hands: 1,
                        effect: None,
                    },
                    BodySlotDef {
                        name: "right_hand".to_string(),
                        capacity: 1,
                        slot_type: SlotType::Grasp,
                        hands: 1,
                        effect: None,
                    },
                ],
                max_health: 100,
                base_max_weight: Some(100),
                stats: HashMap::new(),
                skills: HashMap::new(),
            },
        );
        anatomy
    }

    fn with_login_name(mut hero: Object, login: &str) -> Object {
        hero.set_property_string(crate::object::LOGIN_NAME_PROPERTY, login);
        hero
    }

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

    async fn sample_manager() -> (Arc<Mutex<SessionManager<SqlitePersistence>>>, SlackConfig) {
        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let room = ObjectId::new("room:void-001");
        let north = ObjectId::new("room:north-001");

        let mut hero1 = with_login_name(bare("player:hero-001", "Alice"), "alice");
        hero1.location = Some(room.clone());
        hero1.set_property_string("body_plan", "human");
        let mut hero2 = with_login_name(bare("player:hero-002", "Bob"), "bob");
        hero2.location = Some(room.clone());
        hero2.set_property_string("body_plan", "human");

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

        let manager = SessionManager::open(persistence, human_anatomy())
            .await
            .unwrap();
        let config = SlackConfig {
            world_channel: "C_WORLD".to_string(),
            ..SlackConfig::default()
        };
        (Arc::new(Mutex::new(manager)), config)
    }

    #[tokio::test]
    async fn login_binds_user_to_player() {
        let (manager, config) = sample_manager().await;
        let outcome = dispatch_command(
            Arc::clone(&manager),
            "alice",
            "d_alice",
            "login",
            &config,
        )
        .await;
        assert!(outcome.to_sender.iter().any(|l| l.contains("Welcome")));
        assert!(manager.lock().await.is_connected("alice"));
        assert_eq!(outcome.reply_channel, "d_alice");
    }

    #[tokio::test]
    async fn say_reaches_co_located_player_named_channel_mode() {
        let (manager, config) = sample_manager().await;
        dispatch_command(Arc::clone(&manager), "alice", "d_alice", "login", &config).await;
        dispatch_command(Arc::clone(&manager), "bob", "d_bob", "login", &config).await;

        let outcome = dispatch_command(
            manager,
            "alice",
            "d_alice",
            "say hello there",
            &config,
        )
        .await;
        assert_eq!(outcome.room_audience.len(), 1);
        assert_eq!(outcome.room_audience[0].audience, vec!["bob".to_string()]);
        assert!(outcome.channel[0].0.contains("void-001"));
    }

    #[tokio::test]
    async fn say_posts_to_thread_presence_when_configured() {
        let (manager, mut config) = sample_manager().await;
        config.rooms_channel = Some("C_ROOMS".to_string());
        dispatch_command(Arc::clone(&manager), "alice", "d_alice", "login", &config).await;
        dispatch_command(Arc::clone(&manager), "bob", "d_bob", "login", &config).await;

        let outcome = dispatch_command(
            manager,
            "alice",
            "d_alice",
            "say hello thread",
            &config,
        )
        .await;
        assert_eq!(outcome.channel[0].0, "C_ROOMS:thread:room-void-001");
    }

    #[tokio::test]
    async fn tell_resolves_player_display_name() {
        let (manager, config) = sample_manager().await;
        dispatch_command(Arc::clone(&manager), "alice", "d_alice", "login", &config).await;
        dispatch_command(Arc::clone(&manager), "bob", "d_bob", "login", &config).await;

        let outcome = dispatch_command(
            manager,
            "alice",
            "d_alice",
            "tell Bob secret",
            &config,
        )
        .await;
        assert_eq!(outcome.private.len(), 1);
        assert_eq!(outcome.private[0].0, "bob");
        assert!(outcome.private[0].1.contains("secret"));
    }

    #[tokio::test]
    async fn open_mode_say_reaches_all_connected_players() {
        let (manager, mut config) = sample_manager().await;
        config.play_mode = crate::gateway::PlayMode::Open;
        dispatch_command(Arc::clone(&manager), "alice", "d_alice", "login", &config).await;
        dispatch_command(Arc::clone(&manager), "bob", "d_bob", "login", &config).await;
        dispatch_command(Arc::clone(&manager), "bob", "d_bob", "go north", &config).await;

        let outcome = dispatch_command(
            manager,
            "alice",
            "d_alice",
            "say hello everyone",
            &config,
        )
        .await;
        assert!(outcome.room_audience.is_empty());
        assert!(outcome.channel.iter().any(|(ch, line)| {
            ch == &config.world_channel && line.contains("hello everyone")
        }));
    }

    #[tokio::test]
    async fn open_mode_look_broadcasts_with_location_context() {
        let (manager, mut config) = sample_manager().await;
        config.play_mode = crate::gateway::PlayMode::Open;
        dispatch_command(Arc::clone(&manager), "alice", "d_alice", "login", &config).await;

        let outcome =
            dispatch_command(manager, "alice", "d_alice", "look", &config).await;
        assert!(outcome.channel.iter().any(|(ch, line)| {
            ch == &config.world_channel
                && line.contains("Alice @ The Void")
                && line.contains("Void")
        }));
    }

    #[tokio::test]
    async fn open_mode_movement_skips_room_presence_sync() {
        let (manager, mut config) = sample_manager().await;
        config.play_mode = crate::gateway::PlayMode::Open;
        dispatch_command(Arc::clone(&manager), "alice", "d_alice", "login", &config).await;

        let outcome =
            dispatch_command(manager, "alice", "d_alice", "go north", &config).await;
        assert!(outcome.presence_sync.is_none());
        assert!(outcome.channel.iter().any(|(ch, _)| ch == &config.world_channel));
    }

    #[tokio::test]
    async fn movement_syncs_room_presence() {
        let (manager, config) = sample_manager().await;
        dispatch_command(Arc::clone(&manager), "alice", "d_alice", "login", &config).await;

        let outcome = dispatch_command(manager, "alice", "d_alice", "go north", &config).await;
        let sync = outcome.presence_sync.expect("presence sync");
        assert!(sync.join.iter().any(|c| c.contains("north-001")));
        assert!(sync.part.iter().any(|c| c.contains("void-001")));
    }
}