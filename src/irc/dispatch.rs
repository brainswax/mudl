//! IRC transport adapter: rate limits, login lifecycle, and [`CommandResult`] delivery.
//!
//! Player verbs route through [`CommandDispatcher`](crate::command::CommandDispatcher);
//! this module maps transport-neutral results to [`DispatchOutcome`].

use std::sync::Arc;

use tokio::sync::Mutex;

use crate::command::parse_command_line;
use crate::command::{
    CommandDispatcher, CommandLine, CommandResult, LookOptions, PlayerDispatchOptions,
};
use crate::display::{format_room_look_player, DisplayMode};
use crate::gateway::{
    normalize_nick, normalize_player_display_name, normalize_player_login_name, parse_login_args,
    rate_limit_kind_for_line, resolve_player_for_login, verify_login, display_name_from_login_name,
    LoginRequest, RateLimitKind, SessionManager,
};
use crate::transport::{
    outcome_fields_from_plan, IrcPresenceResolver, IrcTellResolver, MessageRouter,
};
use crate::irc::nick::sanitize_nick_display;
use crate::mudl::AnatomyRegistry;
use crate::object::ObjectFactory;
use crate::persistence::Persistence;

use super::channels::{
    ic_join_notice, login_channel_joins, logout_channel_parts,
};
use super::config::IrcConfig;
use super::nickserv::{
    identify_nick_command, identify_relay_ack, player_help_text, MANUAL_REGISTRATION_HINT,
};
use super::visibility::irc_look_scope;

/// IRC routing instructions produced by command dispatch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DispatchOutcome {
    pub sender: String,
    pub to_sender: Vec<String>,
    pub private: Vec<(String, String)>,
    pub room_audience: Vec<RoomDelivery>,
    pub channel: Vec<(String, String)>,
    pub channel_sync: Option<ChannelSync>,
    /// NickServ PRIVMSG bodies relayed from the bot connection (`IDENTIFY nick pass`, etc.).
    pub nickserv_outbound: Vec<String>,
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

    let mut logged_in = {
        let mgr = manager.lock().await;
        mgr.session_handle(nick).is_some()
    };

    if !logged_in && config.login_auth.auto_login {
        let mut mgr = manager.lock().await;
        if crate::gateway::attempt_auto_login(&mut mgr, nick, &config.login_auth)
            .await
            .is_some()
        {
            logged_in = true;
        }
    }

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
        look: LookOptions::player(irc_look_scope(config.play_mode)),
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
    let mgr = manager.lock().await;
    let actor_label = mgr
        .with_session(nick, |session| {
            session.with_world(|world, player| {
                world
                    .object(player.actor_id())
                    .map(|obj| obj.name.clone())
            })
        })
        .await
        .flatten()
        .unwrap_or_else(|| sender.clone());
    let presence = IrcPresenceResolver { config };
    let router = MessageRouter::new(
        config.play_mode,
        config.open_movement_notices,
        nick,
        &mgr,
    );
    let plan = router
        .plan_command_deliveries(result, &presence, &IrcTellResolver, &actor_label)
        .await;
    drop(mgr);
    outcome_from_plan(sender, &plan)
}

fn outcome_from_plan(sender: String, plan: &crate::transport::DeliveryPlan) -> DispatchOutcome {
    let fields = outcome_fields_from_plan(plan);
    let mut outcome = DispatchOutcome {
        sender,
        to_sender: fields.to_sender,
        persist: fields.persist,
        ..Default::default()
    };
    outcome.private = fields.private;
    for (audience, lines) in fields.room_audience {
        outcome.room_audience.push(RoomDelivery { audience, lines });
    }
    outcome.channel = fields.channel;
    if let Some(sync) = fields.presence_sync {
        outcome.channel_sync = Some(ChannelSync {
            nick: sync.actor,
            join: sync.join,
            part: sync.part,
        });
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
        "register" => {
            dispatch_register(manager, persistence, nick, &line.args, sender, config).await
        }
        "nickserv" | "ns" => {
            dispatch_nickserv_logged_out(nick, sender, &line.args, config)
        }
        "identify" => dispatch_nickserv_identify(nick, sender, &line.args, config),
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

fn dispatch_nickserv_logged_out(
    nick: &str,
    sender: String,
    args: &[String],
    config: &IrcConfig,
) -> DispatchOutcome {
    if args.is_empty() || matches!(args[0].as_str(), "help" | "?") {
        return DispatchOutcome {
            sender,
            to_sender: split_help_lines(&player_help_text(config.identity_policy.is_strict())),
            ..Default::default()
        };
    }
    match args[0].as_str() {
        "identify" => dispatch_nickserv_identify(nick, sender, &args[1..], config),
        "register" => DispatchOutcome {
            sender,
            to_sender: vec![MANUAL_REGISTRATION_HINT.to_string()],
            ..Default::default()
        },
        _ => DispatchOutcome {
            sender,
            to_sender: vec![player_help_text(config.identity_policy.is_strict())],
            ..Default::default()
        },
    }
}

fn dispatch_nickserv_identify(
    nick: &str,
    sender: String,
    args: &[String],
    _config: &IrcConfig,
) -> DispatchOutcome {
    let Some(password) = args.first().map(String::as_str).filter(|s| !s.is_empty()) else {
        return DispatchOutcome {
            sender,
            to_sender: vec![
                "Usage: nickserv identify <password> (or /msg NickServ IDENTIFY <password>)"
                    .to_string(),
            ],
            ..Default::default()
        };
    };
    DispatchOutcome {
        sender,
        to_sender: vec![identify_relay_ack().to_string()],
        nickserv_outbound: vec![identify_nick_command(nick, password)],
        ..Default::default()
    }
}

fn split_help_lines(text: &str) -> Vec<String> {
    text.lines().map(str::to_string).collect()
}

async fn dispatch_login_after_auto_register<P: Persistence + Clone>(
    manager: &mut SessionManager<P>,
    nick: &str,
    sender: String,
    config: &IrcConfig,
) -> DispatchOutcome {
    let room_id = if let Some(handle) = manager.session_handle(nick) {
        let session = handle.lock().await;
        session.current_location().cloned()
    } else {
        None
    };
    DispatchOutcome {
        sender: sender.clone(),
        to_sender: vec![
            "Welcome to MUDL — you're already registered. Type 'help' for commands."
                .to_string(),
        ],
        channel_sync: Some(ChannelSync {
            nick: sender,
            join: login_channel_joins(config, room_id.as_ref()),
            part: Vec::new(),
        }),
        persist: true,
        ..Default::default()
    }
}

async fn dispatch_register<P: Persistence + Clone>(
    manager: &mut SessionManager<P>,
    persistence: &P,
    nick: &str,
    args: &[String],
    sender: String,
    config: &IrcConfig,
) -> DispatchOutcome {
    let (login_name, display_name) = if args.is_empty() {
        let login = match normalize_player_login_name(nick) {
            Ok(name) => name,
            Err(message) => {
                return DispatchOutcome {
                    sender,
                    to_sender: vec![message.to_string()],
                    ..Default::default()
                };
            }
        };
        let display = sanitize_nick_display(nick);
        (login, display)
    } else {
        let raw = args.join(" ");
        let login = match normalize_player_login_name(&raw) {
            Ok(name) => name,
            Err(message) => {
                return DispatchOutcome {
                    sender,
                    to_sender: vec![message.to_string()],
                    ..Default::default()
                };
            }
        };
        let display = match normalize_player_display_name(&raw) {
            Ok(name) => name,
            Err(_) => display_name_from_login_name(&login),
        };
        (login, display)
    };

    let factory = ObjectFactory::new(persistence.clone());
    let anatomy = active_anatomy();
    match manager
        .register_and_login(nick, &login_name, &display_name, &factory, &anatomy)
        .await
    {
        Ok(player_id) => {
            let room_id = if let Some(handle) = manager.session_handle(nick) {
                let session = handle.lock().await;
                session.current_location().cloned()
            } else {
                None
            };
            let outcome = DispatchOutcome {
                sender: sender.clone(),
                to_sender: vec![
                    format!("Welcome to MUDL, {display_name}! Type 'help' for commands."),
                    format!("Login name: {login_name} ({player_id})."),
                ],
                channel_sync: Some(ChannelSync {
                    nick: sender.clone(),
                    join: login_channel_joins(config, room_id.as_ref()),
                    part: Vec::new(),
                }),
                persist: true,
                ..Default::default()
            };
            outcome
        }
        Err(err) => {
            if let crate::gateway::RegisterError::LoginNameTaken { .. } = &err {
                if config.login_auth.auto_login
                    && crate::gateway::attempt_auto_login(manager, nick, &config.login_auth)
                        .await
                        .is_some()
                {
                    return dispatch_login_after_auto_register(manager, nick, sender, config).await;
                }
            }
            DispatchOutcome {
                sender,
                to_sender: vec![err.to_string()],
                ..Default::default()
            }
        }
    }
}

fn active_anatomy() -> AnatomyRegistry {
    crate::command::load_active_universe()
        .ok()
        .and_then(|u| u.active_world().ok().map(|w| w.anatomy.clone()))
        .unwrap_or_default()
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
            let message = if config.login_auth.require_auth {
                "Invalid login credentials.".to_string()
            } else {
                format!(
                    "No player login name matches nick '{nick}'. Try 'register', 'login <player-id>' (e.g. 'login player:brains'), or 'register <login-name>'."
                )
            };
            return DispatchOutcome {
                sender,
                to_sender: vec![message],
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
                    join: {
                        let room_id = if let Some(handle) = manager.session_handle(nick) {
                            let session = handle.lock().await;
                            session.current_location().cloned()
                        } else {
                            None
                        };
                        login_channel_joins(config, room_id.as_ref())
                    },
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
                    outcome
                        .to_sender
                        .push(ic_join_notice(config, &room_id));
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
            let part = logout_channel_parts(config, old_room.as_ref());
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
    let mut lines = vec![
        "Send 'register [login-name]' to create a character (requires a wizard on this world)."
            .to_string(),
        config.login_auth.logged_out_help(),
    ];
    if config.identity_policy.is_strict() || config.nickserv.is_configured() {
        lines.push("NickServ: 'nickserv identify <password>' or 'nickserv help'.".to_string());
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::SessionManager;
    use crate::irc::config::IrcConfig;
    use crate::object::{ContainerSpec, Object, ObjectId, PermissionFlags, ReadableSpec};
    use crate::persistence::SqlitePersistence;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::Mutex;

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

    async fn sample_manager() -> (SqlitePersistence, SessionManager<SqlitePersistence>, IrcConfig) {
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

        let manager = SessionManager::open(persistence.clone(), human_anatomy())
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
    async fn login_binds_nick_to_player_login_name() {
        let (manager, config) = manager_arc().await;
        {
            let mgr = manager.lock().await;
            let mut guard = mgr.world().lock().await;
            let mut hero = guard
                .object(&ObjectId::new("player:hero-001"))
                .cloned()
                .expect("hero");
            hero.id = ObjectId::new("player:alice");
            hero.set_property_string(crate::object::LOGIN_NAME_PROPERTY, "alice");
            guard.cache_object(hero);
        }
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
    async fn auto_login_runs_look_without_explicit_login() {
        let (manager, mut config) = manager_arc().await;
        config.login_auth.auto_login = true;
        let outcome = dispatch_command(manager.clone(), "alice", "look", &config).await;
        assert!(manager.lock().await.is_connected("alice"));
        assert!(
            !outcome.to_sender.is_empty(),
            "look should return room output after auto-login: {:?}",
            outcome.to_sender
        );
        assert!(outcome
            .to_sender
            .iter()
            .any(|l| l.contains("exit") || l.contains("north")));
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
    async fn register_creates_player_when_wizard_exists() {
        let (manager, config) = manager_arc().await;
        let mut wizard = bare("player:wizard-001", "Wizard");
        wizard.permissions = PermissionFlags::wizard_role();
        wizard.owner = ObjectId::new("player:wizard-001");
        {
            let mgr = manager.lock().await;
            mgr.world()
                .lock()
                .await
                .cache_object(wizard);
        }

        let outcome = dispatch_command(
            Arc::clone(&manager),
            "brains",
            "register",
            &config,
        )
        .await;
        assert!(outcome.to_sender.iter().any(|l| l.contains("Welcome")));
        assert!(outcome
            .to_sender
            .iter()
            .any(|l| l.contains("Login name: brains (player:brains)")));
        assert!(manager.lock().await.is_connected("brains"));
    }

    #[tokio::test]
    async fn register_denied_without_wizard_in_world() {
        let (manager, config) = manager_arc().await;
        let outcome = dispatch_command(manager, "alice", "register", &config).await;
        assert!(outcome
            .to_sender
            .iter()
            .any(|l| l.contains("Registration is closed")));
    }

    #[tokio::test]
    async fn open_login_hints_when_nick_does_not_match_player_name() {
        let (manager, config) = manager_arc().await;
        let outcome = dispatch_command(manager, "brains", "login", &config).await;
        assert!(outcome
            .to_sender
            .iter()
            .any(|l| l.contains("No player login name matches nick")));
        assert!(outcome
            .to_sender
            .iter()
            .any(|l| l.contains("login <player-id>")));
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

        let mut sword = bare("item:rusty-sword", "rusty sword");
        sword.location = Some(room_id.clone());

        let mut chest = bare("item:chest-001", "chest");
        chest.apply_container_role(&ContainerSpec {
            capacity: 5,
            open: false,
            ..ContainerSpec::default()
        });
        chest.location = Some(room.clone());

        let mut sign = bare("item:sign-001", "sign");
        sign.apply_readable_role(&ReadableSpec {
            text: "Welcome wanderer.".to_string(),
            writable: false,
        });
        sign.location = Some(room.clone());

        persistence.save_object(&hero1).await.unwrap();
        persistence.save_object(&hero2).await.unwrap();
        persistence.save_object(&place).await.unwrap();
        persistence.save_object(&north_room).await.unwrap();
        persistence.save_object(&sword).await.unwrap();
        persistence.save_object(&chest).await.unwrap();
        persistence.save_object(&sign).await.unwrap();

        let manager = SessionManager::open(persistence, human_anatomy())
            .await
            .unwrap();
        (Arc::new(Mutex::new(manager)), IrcConfig::default())
    }

    #[tokio::test]
    async fn drop_via_dispatcher_persists() {
        let room = ObjectId::new("room:void-001");
        let (manager, config) = manager_with_sword_at(&room).await;
        dispatch_command(Arc::clone(&manager), "alice", "login", &config).await;
        let take = dispatch_command(Arc::clone(&manager), "alice", "take rusty sword", &config).await;
        assert!(take.persist, "take should succeed: {:?}", take.to_sender);

        let outcome = dispatch_command(manager.clone(), "alice", "drop rusty sword", &config).await;
        assert!(outcome.persist);
        assert!(outcome
            .to_sender
            .iter()
            .any(|l| l.contains("drop") && l.contains("rusty sword")));

        let sword_on_ground = manager
            .lock()
            .await
            .with_session("alice", |s| {
                s.with_world(|world, player| {
                    let hand = world
                        .object(player.actor_id())
                        .and_then(|p| p.body_slot_item("right_hand"));
                    let sword = world.object(&ObjectId::new("item:rusty-sword"));
                    (hand.is_none(), sword.and_then(|o| o.location.clone()))
                })
            })
            .await
            .unwrap();
        assert!(sword_on_ground.0, "hand should be empty after drop");
        assert_eq!(
            sword_on_ground.1.as_ref().map(|id| id.as_str()),
            Some("room:void-001")
        );
    }

    async fn manager_with_rat() -> (Arc<Mutex<SessionManager<SqlitePersistence>>>, IrcConfig) {
        use crate::creature::vitality::init_creature_vitality;
        use crate::mudl::PlayerTemplate;

        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let room = ObjectId::new("room:void-001");
        let anatomy = human_anatomy();
        let human = anatomy.creatures.get("human").expect("human anatomy");

        let mut hero = with_login_name(bare("player:hero-001", "Alice"), "alice");
        hero.location = Some(room.clone());
        hero.init_creature_role(&PlayerTemplate {
            name: "Alice".to_string(),
            creature: "human".to_string(),
            gender: "neutral".to_string(),
        });
        init_creature_vitality(&mut hero, human);

        let mut rat = bare("npc:rat-001", "Giant Rat");
        rat.location = Some(room.clone());
        rat.init_creature_role(&PlayerTemplate {
            name: "Giant Rat".to_string(),
            creature: "human".to_string(),
            gender: "neutral".to_string(),
        });
        init_creature_vitality(&mut rat, human);

        let place = bare("room:void-001", "The Void");
        persistence.save_object(&hero).await.unwrap();
        persistence.save_object(&rat).await.unwrap();
        persistence.save_object(&place).await.unwrap();

        let manager = SessionManager::open(persistence, anatomy).await.unwrap();
        (Arc::new(Mutex::new(manager)), IrcConfig::default())
    }

    #[tokio::test]
    async fn attack_via_dispatcher_strikes_room_creature() {
        let (manager, config) = manager_with_rat().await;
        dispatch_command(Arc::clone(&manager), "alice", "login", &config).await;

        let outcome = dispatch_command(manager.clone(), "alice", "attack giant rat", &config).await;
        assert!(outcome.persist, "attack outcome: {:?}", outcome.to_sender);
        assert!(outcome.to_sender.iter().any(|l| {
            l.contains("strike") || l.contains("strikes") || l.contains("damage")
        }));

        let rat_health = manager
            .lock()
            .await
            .with_session("alice", |s| {
                s.with_world(|world, _| {
                    world
                        .object(&ObjectId::new("npc:rat-001"))
                        .map(crate::creature::creature_health)
                })
            })
            .await
            .unwrap();
        assert!(rat_health.unwrap_or(100) < 100);
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
    async fn nickserv_identify_relays_without_echoing_password() {
        let (manager, config) = manager_arc().await;
        let outcome = dispatch_command(
            manager,
            "alice",
            "nickserv identify sekrit-pass",
            &config,
        )
        .await;
        assert_eq!(outcome.nickserv_outbound, vec!["IDENTIFY alice sekrit-pass"]);
        assert!(outcome
            .to_sender
            .iter()
            .any(|l| l.contains("Identification request sent")));
        assert!(!outcome.to_sender.iter().any(|l| l.contains("sekrit-pass")));
    }

    #[tokio::test]
    async fn nickserv_register_directs_to_client() {
        let (manager, config) = manager_arc().await;
        let outcome = dispatch_command(
            manager,
            "alice",
            "nickserv register sekrit-pass alice@example.com",
            &config,
        )
        .await;
        assert!(outcome.nickserv_outbound.is_empty());
        assert!(outcome
            .to_sender
            .iter()
            .any(|l| l.contains("IRC client")));
        assert!(!outcome
            .to_sender
            .iter()
            .any(|l| l.contains("sekrit-pass")));
    }

    #[tokio::test]
    async fn open_mode_say_reaches_all_connected_players() {
        let (manager, mut config) = manager_arc().await;
        config.play_mode = crate::gateway::PlayMode::Open;
        dispatch_command(Arc::clone(&manager), "alice", "login", &config).await;
        dispatch_command(Arc::clone(&manager), "bob", "login", &config).await;
        dispatch_command(Arc::clone(&manager), "bob", "go north", &config).await;

        let outcome =
            dispatch_command(manager, "alice", "say hello everyone", &config).await;
        assert!(outcome.room_audience.is_empty());
        assert!(outcome.channel.iter().any(|(ch, line)| {
            ch == &config.world_channel && line.contains("hello everyone")
        }));
    }

    #[tokio::test]
    async fn open_mode_look_stays_in_current_room() {
        let north = ObjectId::new("room:north-001");
        let (manager, mut config) = manager_with_sword_at(&north).await;
        config.play_mode = crate::gateway::PlayMode::Open;
        dispatch_command(Arc::clone(&manager), "alice", "login", &config).await;

        let outcome = dispatch_command(manager, "alice", "look rusty sword", &config).await;
        assert!(outcome.to_sender.is_empty());
        assert!(outcome.channel.iter().any(|(ch, line)| {
            ch == &config.world_channel
                && line.contains("Alice @ The Void")
                && line.contains("don't see anything like \"rusty sword\"")
        }));
    }

    #[tokio::test]
    async fn open_mode_look_broadcasts_with_location_context() {
        let (manager, mut config) = manager_arc().await;
        config.play_mode = crate::gateway::PlayMode::Open;
        dispatch_command(Arc::clone(&manager), "alice", "login", &config).await;

        let outcome = dispatch_command(manager, "alice", "look", &config).await;
        assert!(outcome.channel.iter().any(|(ch, line)| {
            ch == &config.world_channel
                && line.contains("Alice @ The Void")
                && (line.contains("void") || line.contains("Void"))
        }));
    }

    #[tokio::test]
    async fn open_mode_look_self_broadcasts_with_location_context() {
        let (manager, mut config) = manager_arc().await;
        config.play_mode = crate::gateway::PlayMode::Open;
        dispatch_command(Arc::clone(&manager), "alice", "login", &config).await;

        let outcome = dispatch_command(manager, "alice", "look self", &config).await;
        assert!(outcome.channel.iter().any(|(ch, line)| {
            ch == &config.world_channel
                && line.contains("Alice @ The Void")
                && (line.contains("holding") || line.contains("wearing"))
        }));
    }

    #[tokio::test]
    async fn open_mode_take_broadcasts_with_location_context() {
        let room = ObjectId::new("room:void-001");
        let (manager, mut config) = manager_with_sword_at(&room).await;
        config.play_mode = crate::gateway::PlayMode::Open;
        dispatch_command(Arc::clone(&manager), "alice", "login", &config).await;

        let outcome = dispatch_command(manager, "alice", "take rusty sword", &config).await;
        assert!(outcome.persist);
        assert!(outcome.channel.iter().any(|(ch, line)| {
            ch == &config.world_channel
                && line.contains("Alice @ The Void")
                && line.contains("pick up")
                && line.contains("rusty sword")
        }));
    }

    #[tokio::test]
    async fn open_mode_open_broadcasts_with_location_context() {
        let room = ObjectId::new("room:void-001");
        let (manager, mut config) = manager_with_sword_at(&room).await;
        config.play_mode = crate::gateway::PlayMode::Open;
        dispatch_command(Arc::clone(&manager), "alice", "login", &config).await;

        let outcome = dispatch_command(manager, "alice", "open chest", &config).await;
        assert!(outcome.persist);
        assert!(outcome.channel.iter().any(|(ch, line)| {
            ch == &config.world_channel
                && line.contains("Alice @ The Void")
                && line.contains("open the chest")
        }));
    }

    #[tokio::test]
    async fn open_mode_examine_broadcasts_with_location_context() {
        let room = ObjectId::new("room:void-001");
        let (manager, mut config) = manager_with_sword_at(&room).await;
        config.play_mode = crate::gateway::PlayMode::Open;
        dispatch_command(Arc::clone(&manager), "alice", "login", &config).await;

        let outcome = dispatch_command(manager, "alice", "examine rusty sword", &config).await;
        assert!(outcome.channel.iter().any(|(ch, line)| {
            ch == &config.world_channel
                && line.contains("Alice @ The Void")
                && line.contains("rusty sword")
        }));
    }

    #[tokio::test]
    async fn open_mode_read_broadcasts_with_location_context() {
        let room = ObjectId::new("room:void-001");
        let (manager, mut config) = manager_with_sword_at(&room).await;
        config.play_mode = crate::gateway::PlayMode::Open;
        dispatch_command(Arc::clone(&manager), "alice", "login", &config).await;

        let outcome = dispatch_command(manager, "alice", "read sign", &config).await;
        assert!(outcome.channel.iter().any(|(ch, line)| {
            ch == &config.world_channel
                && line.contains("Alice @ The Void")
                && line.contains("Welcome wanderer")
        }));
    }

    #[tokio::test]
    async fn open_mode_movement_skips_room_channel_sync() {
        let (manager, mut config) = manager_arc().await;
        config.play_mode = crate::gateway::PlayMode::Open;
        dispatch_command(Arc::clone(&manager), "alice", "login", &config).await;

        let outcome = dispatch_command(manager, "alice", "go north", &config).await;
        assert!(outcome.channel_sync.is_none());
        assert!(outcome.channel.iter().any(|(ch, line)| {
            ch == &config.world_channel
                && line.contains("Alice @ North")
                && line.contains("head north")
        }));
        assert!(
            !outcome
                .channel
                .iter()
                .any(|(_, line)| line.contains("has left") || line.contains("has arrived"))
        );
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