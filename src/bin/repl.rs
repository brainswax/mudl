use std::collections::HashMap;

use anyhow::Result;
use rustyline::{error::ReadlineError, DefaultEditor};

use mudl::gateway::ensure_bootstrap_wizard;
use mudl::command::{
    apply_set, apply_trigger_add, apply_trigger_clear, apply_trigger_remove, apply_trigger_set,
    apply_unset, bootstrap_active_universe, create_at_location_with_options,
    create_key_for_container, format_trigger_list, narrate_trigger_added,
    narrate_trigger_cleared, narrate_trigger_removed, narrate_trigger_set,
    narrate_trigger_test_empty, package_module, parse_command_line, parse_create_command,
    parse_dig_command, parse_link_command, parse_set_command, parse_trigger_command,
    parse_unlink_command, parse_unset_command, preview_trigger_test, reload_universe,
    resolve_container_target, resolve_trigger_target_name, soft_delete_object, trigger_command_help,
    undelete_object, validate_trigger_host, CommandDispatcher, CommandResult, LookOptions,
    TriggerCommand, TriggerError,
};
use mudl::creature::{
    add_behavior_template, damage_creature, format_creature_behavior_list,
    heal_creature, parse_vital_amount_args, DEFAULT_DAMAGE_AMOUNT, DEFAULT_HEAL_AMOUNT,
};
use mudl::display::{
    format_examine_output, format_no_parent_message, narrate_create, narrate_dig,
    narrate_field_set, narrate_field_unset, narrate_link, narrate_loaded, narrate_module_bundled,
    narrate_module_reloaded, narrate_no_location, narrate_no_location_builder,
    narrate_not_in_cache, narrate_saved, narrate_target_not_found, narrate_wizard_not_found,
    parse_examine_request, resolve_examine_request, resolve_target, Describable, DisplayContext,
    DisplayMode, ExamineError, ExamineResolution, ResolveScope, TargetResolution,
};
use mudl::inventory::{
    break_item, close_container, harvest_item, lock_container,
    open_container, parse_put_args, parse_unlock_args, put_item, read_item, remove_item,
    unlock_container, use_item, wear_item, wield_item,
};
use mudl::mudl::{default_module_dir, LoadedUniverse};
use mudl::object::{Object, ObjectFactory, ObjectId};
use mudl::persistence::{
    Persistence, SqlitePersistence, WriterGuard, WriterLockOptions, WriterMode,
};
use mudl::repl::Session;
use mudl::world::{exit_index, movement_from_line};
use mudl::world::place_builder::DigRequest;
use tracing::{error, info, warn};

async fn resolve_in_session(
    session: &mut Session,
    persistence: &SqlitePersistence,
    target: Option<&str>,
) -> Result<TargetResolution> {
    let resolution = if let Some(name) = target {
        session.resolve_target(name, ResolveScope::General)
    } else if let Some(loc) = session.current_location() {
        TargetResolution::Found(loc.clone())
    } else {
        TargetResolution::NotFound
    };

    if let TargetResolution::Found(ref id) = resolution {
        session.ensure_object(persistence, id).await?;
    }

    Ok(resolution)
}

async fn persist_session(session: &mut Session, persistence: &SqlitePersistence) -> Result<()> {
    let _ = session.persist_changes(persistence).await?;
    Ok(())
}

fn print_command_result(result: &CommandResult) {
    for line in &result.lines_to_actor {
        println!("{line}");
    }
}

fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
}

fn location_object<'a>(
    loc_id: &ObjectId,
    objects: &'a HashMap<ObjectId, Object>,
) -> Option<&'a Object> {
    objects.get(loc_id)
}

fn render_object(obj: &Object, ctx: &DisplayContext, detailed: bool, debug: bool) {
    let output = if debug {
        obj.dump()
    } else if detailed {
        obj.describe_detailed(ctx)
    } else {
        obj.describe(ctx)
    };
    println!("{output}");
}

async fn run_look_command(
    session: &mut Session,
    persistence: &SqlitePersistence,
    target: Option<&str>,
    builder: bool,
) -> Result<(), anyhow::Error> {
    let options = if builder {
        LookOptions::builder()
    } else {
        LookOptions::player(ResolveScope::General)
    };
    let result = CommandDispatcher::look_async(session, persistence, target, options).await;
    print_command_result(&result);
    Ok(())
}

async fn run_examine_command(
    session: &mut Session,
    persistence: &SqlitePersistence,
    args: &[&str],
    mode: DisplayMode,
    builder: bool,
) -> Result<(), anyhow::Error> {
    let request = parse_examine_request(args);
    let ctx = session.display_context(mode.clone());

    match resolve_examine_request(
        &request,
        &session.anatomy(),
        session.player_id(),
        session.current_location(),
        &ctx.objects,
    ) {
        Ok(resolution) => {
            if let ExamineResolution::Prototype { prototype_id, .. } = &resolution {
                session.ensure_object(persistence, prototype_id).await?;
            }
            let ctx = session.display_context(mode);
            if let Some(output) = format_examine_output(&resolution, &ctx) {
                println!("{output}");
            } else if let ExamineResolution::Object(id) = resolution {
                if let Some(obj) = ctx.objects.get(&id) {
                    render_object(obj, &ctx, builder, false);
                } else {
                    println!(
                        "{}",
                        narrate_target_not_found(args.first().unwrap_or(&"target"))
                    );
                }
            }
        }
        Err(ExamineError::Ambiguous(msg)) => println!("{msg}"),
        Err(ExamineError::NoParent(id)) => {
            if let Some(obj) = ctx.objects.get(&id) {
                println!("{}", format_no_parent_message(obj));
            } else {
                println!(
                    "{}",
                    narrate_target_not_found(args.first().unwrap_or(&"target"))
                );
            }
        }
        Err(ExamineError::NotFound) => {
            if args.is_empty() {
                println!(
                    "{}",
                    if builder {
                        narrate_no_location_builder("Try '@examine <target>' or '@examine here'.")
                    } else {
                        narrate_no_location()
                    }
                );
            } else {
                println!("{}", narrate_target_not_found(args[0]));
            }
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    mudl::env::load_project_env();
    init_tracing();

    let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "repl.db".to_string());

    info!("MUDL REPL starting");
    let writer_options = WriterLockOptions::from_env(WriterMode::Repl);
    let _writer_guard = WriterGuard::acquire(&db_url, &writer_options).map_err(anyhow::Error::from)?;
    if let Some(record) = _writer_guard.record() {
        info!(
            mode = record.mode.as_str(),
            pid = record.pid,
            "acquired single-writer database lock (SEC-23)"
        );
    }
    let persistence = SqlitePersistence::new(&db_url).await?;
    let factory = ObjectFactory::new(persistence.clone());
    let player_id = ObjectId::new(
        std::env::var("DEFAULT_PLAYER").unwrap_or_else(|_| "player:admin-001".to_string()),
    );

    info!(database = %db_url, player = %player_id, "session configuration");

    let module_dir = default_module_dir();
    info!(module = %module_dir.display(), "loading universe module");

    let mut loaded_universe: LoadedUniverse = reload_universe(
        module_dir
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("invalid module path"))?,
    )?;
    let active_world = loaded_universe.active_world()?;
    let mut active_anatomy = active_world.anatomy.clone();
    if active_anatomy.creature("human").is_some() {
        info!(
            universe = %loaded_universe.name,
            world = %active_world.name,
            sources = active_world.sources.len(),
            "universe loaded"
        );
    } else {
        warn!("human creature definition not found in active world");
    }

    let mut bootstrap_location: Option<ObjectId> = None;
    match bootstrap_active_universe(&factory, player_id.clone()).await {
        Ok((universe, loc_id)) => {
            loaded_universe = universe;
            active_anatomy = loaded_universe.active_world()?.anatomy.clone();
            bootstrap_location = Some(loc_id.clone());
            info!(location = %loc_id, "world bootstrapped");
        }
        Err(e) => {
            warn!(error = %e, "bootstrap failed");
        }
    }

    match ensure_bootstrap_wizard(
        &factory,
        &player_id,
        &active_anatomy,
        bootstrap_location.clone(),
    )
    .await
    {
        Ok(true) => info!(
            player = %player_id,
            "created or upgraded bootstrap wizard player from DEFAULT_PLAYER"
        ),
        Ok(false) => {}
        Err(err) => warn!(error = %err, "bootstrap wizard setup failed"),
    }

    let mut session = match Session::restore(
        &persistence,
        player_id.clone(),
        bootstrap_location,
        active_anatomy.clone(),
    )
    .await
    {
        Ok(session) => {
            info!(
                objects = session.len(),
                location = ?session.current_location(),
                "session restored"
            );
            session
        }
        Err(e) => {
            warn!(error = %e, "failed to restore session");
            Session::restore(
                &persistence,
                player_id.clone(),
                None,
                active_anatomy.clone(),
            )
            .await?
        }
    };

    println!("Welcome to MUDL.");
    println!("Type 'help' for commands.");

    let mut rl = DefaultEditor::new()?;
    let history_path = if let Ok(home) = std::env::var("HOME") {
        std::path::PathBuf::from(home).join(".mudl_history")
    } else if let Ok(userprofile) = std::env::var("USERPROFILE") {
        std::path::PathBuf::from(userprofile).join(".mudl_history")
    } else {
        std::path::PathBuf::from(".mudl_history")
    };

    if rl.load_history(&history_path).is_err() {
        // No history file yet — that's fine on first run
    }

    loop {
        let readline = rl.readline("> ");
        match readline {
            Ok(line) => {
                let input = line.trim();
                if input.is_empty() {
                    continue;
                }

                let _ = rl.add_history_entry(line.as_str());

                let parsed = parse_command_line(input);

                let parts: Vec<&str> = input.split_whitespace().collect();
                let cmd = parts[0];

                if parsed.is_meta {
                    if let Err(err) = session.authorize_meta(&parsed.verb) {
                        println!("{err}");
                        continue;
                    }
                } else if let Err(err) =
                    session.authorize_plain(cmd, parts.get(1).copied())
                {
                    println!("{err}");
                    continue;
                }

                if cmd == "go" && parts.len() < 2 {
                    println!("Usage: go <direction>  (or just: north, around, in, …)");
                    continue;
                }
                let exit_index = session
                    .current_location()
                    .and_then(|loc| session.object(loc))
                    .map(|place| exit_index::ExitIndex::from_place(&place));
                if movement_from_line(cmd, &parts[1..], exit_index.as_ref()).is_some() {
                    let movement_line = mudl::command::CommandLine {
                        is_meta: false,
                        verb: parsed.verb.clone(),
                        args: parsed.args.clone(),
                    };
                    let result =
                        CommandDispatcher::movement_async(&mut session, &movement_line).await;
                    print_command_result(&result);
                    if result.persist_world {
                        if let Err(e) = persist_session(&mut session, &persistence).await {
                            error!(error = %e, "persist after go failed");
                        }
                    }
                    continue;
                }

                match cmd {
                    "help" => {
                        println!("Commands:");
                        println!("  create <type> <name...>     - e.g. create sword Rusty Sword");
                        println!("  list                        - list objects in session memory");
                        println!("  look [target]  (l)          - in-character brief view");
                        println!("  @look [target]              - wizard: structured builder view");
                        println!(
                            "  examine [target]  (x)       - in-character detail (self, .body)"
                        );
                        println!(
                            "  @examine [target] [parent]  - wizard: properties, anatomy, prototype"
                        );
                        println!(
                            "  @dump [target]              - wizard: full JSON dump of an object"
                        );
                        println!(
                            "  inventory  (i)              - show hands, pockets, and containers"
                        );
                        println!("  get/take <item>             - pick up an item from the room");
                        println!("  drop <item>                 - drop a carried item");
                        println!(
                            "  put [count] <item> in <container> - stow items in hand or nearby containers"
                        );
                        println!(
                            "  remove <item> from <container> - take an item out of a container"
                        );
                        println!("  wield <item>                - hold/wield an item in your hand");
                        println!(
                            "  read <object>               - read text on a note, sign, or mailbox"
                        );
                        println!(
                            "  open/close <container|door|window> - open or close a container or portal"
                        );
                        println!(
                            "  lock/unlock <container|door|window> [with <key>] - lock or unlock (auto-finds key)"
                        );
                        println!("  wear <item>                 - wear a container or garment");
                        println!(
                            "  attack <creature>           - strike a creature (turn-based combat)"
                        );
                        println!(
                            "  go <dir>  (or n/s/e/w/around/…) - move; shows room description and exits"
                        );
                        println!(
                            "  @set <target> <key> <value>  - wizard: set property/state/verb"
                        );
                        println!("  @unset <target> <key>        - wizard: remove property/verb");
                        println!("  load <id>                   - load object from persistence");
                        println!("  save <id>                   - save object from session");
                        println!("  module reload               - reload MUDL module from disk");
                        println!(
                            "  module bundle <outdir>      - package module to output directory"
                        );
                        println!(
                            "  @create <type> <name...> [key=value...] - wizard create with roles"
                        );
                        println!(
                            "  @keyfor <container> [name]  - wizard: create a key for a container"
                        );
                        println!("  @delete <target>            - wizard: soft-delete an object");
                        println!(
                            "  @damage <creature> [amount] - wizard: apply damage to a creature"
                        );
                        println!("  @heal <creature> [amount]   - wizard: heal a creature");
                        println!(
                            "  @addbehavior <creature> <template> - wizard: attach behavior template"
                        );
                        println!("  @listbehaviors <creature>   - wizard: list creature behaviors");
                        println!(
                            "  @trigger …                  - wizard: attach/list/edit event scripts"
                        );
                        println!(
                            "  @undelete <id>              - wizard: restore soft-deleted object"
                        );
                        println!(
                            "  @dig <dir> <name...>        - wizard: create and link a new place"
                        );
                        println!(
                            "  @link <dir> <target>        - wizard: link exit from here (reciprocal by default)"
                        );
                        println!(
                            "  @unlink <dir>               - wizard: remove an exit from here"
                        );
                        println!("  exit                        - quit");
                    }
                    "@create" | "create" => {
                        let parsed = match parse_create_command(input) {
                            Ok(parsed) => parsed,
                            Err(e) => {
                                println!("{e}");
                                continue;
                            }
                        };
                        match create_at_location_with_options(
                            &factory,
                            &parsed.type_name,
                            &parsed.display_name,
                            player_id.clone(),
                            session.current_location(),
                            &session.anatomy(),
                            parsed.options,
                        )
                        .await
                        {
                            Ok(obj) => {
                                info!(
                                    id = %obj.id,
                                    name = %obj.name,
                                    location = ?obj.location,
                                    roles = ?obj.roles(),
                                    "object created"
                                );
                                let graph = session.objects();
                                let loc = obj
                                    .location
                                    .as_ref()
                                    .and_then(|id| location_object(id, &graph));
                                println!("{}", narrate_create(&obj, loc));
                                session.upsert_object(obj);
                            }
                            Err(e) => {
                                error!(error = %e, "create failed");
                                println!("Your conjuration fizzles.");
                            }
                        }
                    }
                    "list" => {
                        if session.is_empty() {
                            println!("Your working memory is empty.");
                        } else {
                            let names: Vec<String> = session
                                .objects()
                                .values()
                                .map(|obj| obj.name.clone())
                                .collect();
                            println!("You recall: {}", names.join(", "));
                            for (id, obj) in session.objects() {
                                info!(id = %id, name = %obj.name, "session object");
                            }
                        }
                    }
                    "look" | "l" => {
                        if let Err(e) = run_look_command(
                            &mut session,
                            &persistence,
                            parts.get(1).copied(),
                            false,
                        )
                        .await
                        {
                            error!(error = %e, "look failed");
                            println!(
                                "Something stirs in the void, but you cannot make sense of it."
                            );
                        }
                    }
                    "@look" => {
                        if let Err(e) = run_look_command(
                            &mut session,
                            &persistence,
                            parts.get(1).copied(),
                            true,
                        )
                        .await
                        {
                            error!(error = %e, "@look failed");
                            println!("The builder view remains obscured.");
                        }
                    }
                    "examine" | "x" => {
                        if let Err(e) = run_examine_command(
                            &mut session,
                            &persistence,
                            &parts[1..],
                            DisplayMode::Player,
                            false,
                        )
                        .await
                        {
                            error!(error = %e, "examine failed");
                            println!("You study it, but learn nothing new.");
                        }
                    }
                    "@examine" => {
                        if let Err(e) = run_examine_command(
                            &mut session,
                            &persistence,
                            &parts[1..],
                            DisplayMode::Builder,
                            true,
                        )
                        .await
                        {
                            error!(error = %e, "@examine failed");
                            println!("The internal details remain obscured.");
                        }
                    }
                    "@dump" => {
                        let target = parts.get(1).copied();
                        match resolve_in_session(&mut session, &persistence, target).await {
                            Ok(TargetResolution::Found(id)) => {
                                if let Some(obj) = session.object(&id) {
                                    println!("{}", obj.dump());
                                } else if let Some(target) = parts.get(1) {
                                    println!("{}", narrate_target_not_found(target));
                                } else {
                                    println!(
                                        "{}",
                                        narrate_no_location_builder("Use '@dump <target>'.")
                                    );
                                }
                            }
                            Ok(TargetResolution::Ambiguous(msg)) => println!("{msg}"),
                            Ok(TargetResolution::NotFound) => {
                                if parts.get(1).is_some() {
                                    println!("{}", narrate_target_not_found(parts.get(1).unwrap()));
                                } else {
                                    println!(
                                        "{}",
                                        narrate_no_location_builder("Use '@dump <target>'.")
                                    );
                                }
                            }
                            Err(e) => {
                                error!(error = %e, "@dump failed");
                                println!("The underlying structure remains hidden.");
                            }
                        }
                    }
                    "attack" => {
                        if parts.len() < 2 {
                            println!("Usage: attack <creature>");
                            continue;
                        }
                        let target = parts[1..].join(" ");
                        let result =
                            CommandDispatcher::attack_async(&mut session, Some(&target)).await;
                        print_command_result(&result);
                        if result.persist_world {
                            if let Err(e) = persist_session(&mut session, &persistence).await {
                                error!(error = %e, "persist after attack failed");
                            }
                        }
                    }
                    "@damage" => {
                        let rest = parts[1..].join(" ");
                        match parse_vital_amount_args(&rest, DEFAULT_DAMAGE_AMOUNT) {
                            Ok(req) => {
                                match session.with_inventory(|ctx| {
                                    damage_creature(
                                        ctx.player_id,
                                        ctx.room_id,
                                        ctx.objects,
                                        ctx.anatomy,
                                        ctx.dirty.as_deref_mut(),
                                        &req.target_name,
                                        req.amount,
                                    )
                                }) {
                                    Ok(msg) => {
                                        println!("{msg}");
                                        if let Err(e) =
                                            persist_session(&mut session, &persistence).await
                                        {
                                            error!(error = %e, "persist after damage failed");
                                        }
                                    }
                                    Err(e) => println!("{e}"),
                                }
                            }
                            Err(e) => println!("{e}"),
                        }
                    }
                    "@heal" => {
                        let rest = parts[1..].join(" ");
                        match parse_vital_amount_args(&rest, DEFAULT_HEAL_AMOUNT) {
                            Ok(req) => {
                                match session.with_inventory(|ctx| {
                                    heal_creature(
                                        ctx.player_id,
                                        ctx.room_id,
                                        ctx.objects,
                                        ctx.anatomy,
                                        ctx.dirty.as_deref_mut(),
                                        &req.target_name,
                                        req.amount,
                                    )
                                }) {
                                    Ok(msg) => {
                                        println!("{msg}");
                                        if let Err(e) =
                                            persist_session(&mut session, &persistence).await
                                        {
                                            error!(error = %e, "persist after heal failed");
                                        }
                                    }
                                    Err(e) => println!("{e}"),
                                }
                            }
                            Err(e) => println!("{e}"),
                        }
                    }
                    "@addbehavior" => {
                        if parts.len() < 3 {
                            println!("Usage: @addbehavior <creature> <template>");
                            continue;
                        }
                        let creature_name = parts[1];
                        let template_name = parts[2];
                        let world = match loaded_universe.active_world() {
                            Ok(world) => world,
                            Err(e) => {
                                println!("{e}");
                                continue;
                            }
                        };
                        let template = world
                            .behavior_template_defs
                            .iter()
                            .find(|t| t.base_name == template_name);
                        let Some(template) = template else {
                            println!("Unknown behavior template '{template_name}'.");
                            continue;
                        };
                        match session.resolve_target(creature_name, ResolveScope::General) {
                            TargetResolution::Found(id) => {
                                let Some(creature) = session.object(&id) else {
                                    println!("{}", narrate_wizard_not_found());
                                    continue;
                                };
                                if !creature.has_creature_role() {
                                    println!("{} is not a creature.", creature.name);
                                    continue;
                                }
                                let creature_name = creature.name.clone();
                                match session
                                    .object_mut(&id, |creature| add_behavior_template(creature, template))
                                {
                                    Some(true) => {
                                        println!(
                                            "Attached behavior template '{template_name}' to {creature_name}."
                                        );
                                        if let Err(e) =
                                            persist_session(&mut session, &persistence).await
                                        {
                                            error!(error = %e, "persist after addbehavior failed");
                                        }
                                    }
                                    Some(false) => {
                                        println!(
                                            "{creature_name} already has behavior template '{template_name}'."
                                        );
                                    }
                                    None => println!("{}", narrate_wizard_not_found()),
                                }
                            }
                            TargetResolution::NotFound => {
                                println!("{}", narrate_wizard_not_found());
                            }
                            TargetResolution::Ambiguous(msg) => println!("{msg}"),
                        }
                    }
                    "@listbehaviors" => {
                        if parts.len() < 2 {
                            println!("Usage: @listbehaviors <creature>");
                            continue;
                        }
                        let creature_name = parts[1];
                        match session.resolve_target(creature_name, ResolveScope::General) {
                            TargetResolution::Found(id) => {
                                if let Some(creature) = session.object(&id) {
                                    println!("{}", format_creature_behavior_list(&creature));
                                } else {
                                    println!("{}", narrate_wizard_not_found());
                                }
                            }
                            TargetResolution::NotFound => {
                                println!("{}", narrate_wizard_not_found());
                            }
                            TargetResolution::Ambiguous(msg) => println!("{msg}"),
                        }
                    }
                    "@trigger" => {
                        let trigger_cmd = match parse_trigger_command(&parsed.args) {
                            Ok(cmd) => cmd,
                            Err(e) => {
                                println!("{e}");
                                continue;
                            }
                        };
                        if matches!(trigger_cmd, TriggerCommand::Help) {
                            println!("{}", trigger_command_help());
                            continue;
                        }
                        let target_name = match &trigger_cmd {
                            TriggerCommand::List { target }
                            | TriggerCommand::Add { target, .. }
                            | TriggerCommand::Remove { target, .. }
                            | TriggerCommand::Clear { target, .. }
                            | TriggerCommand::Set { target, .. }
                            | TriggerCommand::Test { target, .. } => target.clone(),
                            TriggerCommand::Help => unreachable!(),
                        };
                        let resolved = resolve_trigger_target_name(
                            &target_name,
                            session.current_location(),
                            session.player_id(),
                        );
                        if resolved == "here" {
                            println!("{}", narrate_no_location_builder("Specify a target or stand in a place."));
                            continue;
                        }
                        match resolve_in_session(&mut session, &persistence, Some(&resolved)).await
                        {
                            Ok(TargetResolution::Found(id)) => {
                                let mut obj = match session.object(&id) {
                                    Some(obj) => obj,
                                    None => {
                                        println!("{}", narrate_wizard_not_found());
                                        continue;
                                    }
                                };
                                if let Err(e) = validate_trigger_host(&obj) {
                                    println!("{e}");
                                    continue;
                                }
                                let read_only = matches!(
                                    &trigger_cmd,
                                    TriggerCommand::List { .. } | TriggerCommand::Test { .. }
                                );
                                let outcome = match trigger_cmd {
                                    TriggerCommand::List { .. } => {
                                        println!("{}", format_trigger_list(&obj));
                                        Ok(())
                                    }
                                    TriggerCommand::Add { event, code, .. } => {
                                        match apply_trigger_add(&mut obj, &event, &code) {
                                            Ok(()) => {
                                                println!("{}", narrate_trigger_added(&obj, &event, &code));
                                                Ok(())
                                            }
                                            Err(e) => Err(e),
                                        }
                                    }
                                    TriggerCommand::Remove { event, index, .. } => {
                                        match apply_trigger_remove(&mut obj, &event, index) {
                                            Ok(removed) => {
                                                println!(
                                                    "{}",
                                                    narrate_trigger_removed(&obj, &event, &removed)
                                                );
                                                Ok(())
                                            }
                                            Err(e) => Err(e),
                                        }
                                    }
                                    TriggerCommand::Clear { event, .. } => {
                                        let count = apply_trigger_clear(&mut obj, event.as_deref());
                                        println!(
                                            "{}",
                                            narrate_trigger_cleared(&obj, count, event.as_deref())
                                        );
                                        Ok(())
                                    }
                                    TriggerCommand::Set {
                                        event,
                                        index,
                                        code,
                                        ..
                                    } => match apply_trigger_set(&mut obj, &event, index, &code) {
                                        Ok(()) => {
                                            println!(
                                                "{}",
                                                narrate_trigger_set(&obj, &event, index, &code)
                                            );
                                            Ok(())
                                        }
                                        Err(e) => Err(e),
                                    },
                                    TriggerCommand::Test { event, .. } => {
                                        let lines = preview_trigger_test(&obj, &event);
                                        if lines.is_empty() {
                                            println!("{}", narrate_trigger_test_empty(&obj, &event));
                                        } else {
                                            for line in lines {
                                                println!("{line}");
                                            }
                                        }
                                        Ok(())
                                    }
                                    TriggerCommand::Help => unreachable!(),
                                };
                                match outcome {
                                    Ok(()) => {
                                        if !read_only {
                                            if let Err(e) =
                                                session.persist_object(&persistence, obj).await
                                            {
                                                error!(error = %e, "@trigger save failed");
                                                println!("The change fades before it can take hold.");
                                            }
                                        }
                                    }
                                    Err(TriggerError::NotFound(msg) | TriggerError::Validation(msg)) => {
                                        println!("{msg}");
                                    }
                                    Err(TriggerError::Usage(msg)) => println!("{msg}"),
                                }
                            }
                            Ok(TargetResolution::Ambiguous(msg)) => println!("{msg}"),
                            Ok(TargetResolution::NotFound) => {
                                println!("{}", narrate_target_not_found(&resolved));
                            }
                            Err(e) => {
                                error!(error = %e, "@trigger resolve failed");
                                println!("You cannot reach that object to change it.");
                            }
                        }
                    }
                    "@delete" => {
                        if parts.len() < 2 {
                            println!("Usage: @delete <target>");
                            continue;
                        }
                        let target = parts[1..].join(" ");
                        match resolve_target(
                            &target,
                            session.current_location(),
                            Some(session.player_id()),
                            &session.objects(),
                        ) {
                            Some(id) => {
                                let mut scratch = HashMap::new();
                                match soft_delete_object(&persistence, &id, &mut scratch).await
                                {
                                    Ok(msg) => {
                                        if let Some(obj) = scratch.remove(&id) {
                                            session.upsert_object(obj);
                                        }
                                        println!("{msg}");
                                    }
                                    Err(e) => {
                                        error!(error = %e, "soft delete failed");
                                        println!("The unraveling fails — something resists.");
                                    }
                                }
                            }
                            None => println!("{}", narrate_wizard_not_found()),
                        }
                    }
                    "@undelete" => {
                        if parts.len() < 2 {
                            println!("Usage: @undelete <id>");
                            continue;
                        }
                        let id = ObjectId::new(parts[1]);
                        let mut scratch = HashMap::new();
                        match undelete_object(&persistence, &id, &mut scratch).await {
                            Ok(msg) => {
                                if let Some(obj) = scratch.remove(&id) {
                                    session.upsert_object(obj);
                                }
                                println!("{msg}");
                            }
                            Err(e) => {
                                error!(error = %e, id = %id, "undelete failed");
                                println!("Restoration fails — the threads won't reweave.");
                            }
                        }
                    }
                    "inventory" | "i" => {
                        let result = CommandDispatcher::inventory_async(&session).await;
                        print_command_result(&result);
                    }
                    "get" | "take" => {
                        if parts.len() < 2 {
                            println!("Usage: get <item>");
                            continue;
                        }
                        let item_name = parts[1..].join(" ");
                        let result =
                            CommandDispatcher::take_async(&mut session, Some(&item_name)).await;
                        print_command_result(&result);
                        if result.persist_world {
                            if let Err(e) = persist_session(&mut session, &persistence).await {
                                error!(error = %e, "persist after take failed");
                            }
                        }
                    }
                    "drop" => {
                        if parts.len() < 2 {
                            println!("Usage: drop [count] <item>");
                            continue;
                        }
                        let item_name = parts[1..].join(" ");
                        let result =
                            CommandDispatcher::drop_async(&mut session, Some(&item_name)).await;
                        print_command_result(&result);
                        if result.persist_world {
                            if let Err(e) = persist_session(&mut session, &persistence).await {
                                error!(error = %e, "persist after drop failed");
                            }
                        }
                    }
                    "put" => {
                        let rest = parts[1..].join(" ");
                        match parse_put_args(&rest) {
                            Ok(req) => {
                                match session.with_inventory(|ctx| {
                                    put_item(
                                        ctx,
                                        &req.item_name,
                                        &req.container_name,
                                        req.quantity,
                                    )
                                }) {
                                    Ok(msg) => {
                                        println!("{msg}");
                                        if let Err(e) =
                                            persist_session(&mut session, &persistence).await
                                        {
                                            error!(error = %e, "persist after put failed");
                                        }
                                    }
                                    Err(e) => println!("{e}"),
                                }
                            }
                            Err(e) => println!("{e}"),
                        }
                    }
                    "remove" => {
                        let rest = parts[1..].join(" ");
                        if let Some((item, container)) = rest.split_once(" from ") {
                            match session.with_inventory(|ctx| {
                                remove_item(ctx, item.trim(), container.trim())
                            }) {
                                Ok(msg) => {
                                    println!("{msg}");
                                    if let Err(e) =
                                        persist_session(&mut session, &persistence).await
                                    {
                                        error!(error = %e, "persist after remove failed");
                                    }
                                }
                                Err(e) => println!("{e}"),
                            }
                        } else {
                            println!("Usage: remove <item> from <container>");
                        }
                    }
                    "open" => {
                        if parts.len() < 2 {
                            println!("Usage: open <container>");
                            continue;
                        }
                        let container_name = parts[1..].join(" ");
                        match session.with_inventory(|ctx| open_container(ctx, &container_name)) {
                            Ok(msg) => {
                                println!("{msg}");
                                if let Err(e) = persist_session(&mut session, &persistence).await {
                                    error!(error = %e, "persist after open failed");
                                }
                            }
                            Err(e) => println!("{e}"),
                        }
                    }
                    "close" => {
                        if parts.len() < 2 {
                            println!("Usage: close <container>");
                            continue;
                        }
                        let container_name = parts[1..].join(" ");
                        match session.with_inventory(|ctx| close_container(ctx, &container_name)) {
                            Ok(msg) => {
                                println!("{msg}");
                                if let Err(e) = persist_session(&mut session, &persistence).await {
                                    error!(error = %e, "persist after close failed");
                                }
                            }
                            Err(e) => println!("{e}"),
                        }
                    }
                    "read" => {
                        if parts.len() < 2 {
                            println!("Usage: read <object>");
                            continue;
                        }
                        let item_name = parts[1..].join(" ");
                        match session.with_inventory(|ctx| read_item(ctx, &item_name)) {
                            Ok(msg) => println!("{msg}"),
                            Err(e) => println!("{e}"),
                        }
                    }
                    "break" | "smash" => {
                        if parts.len() < 2 {
                            println!("Usage: break <item>");
                            continue;
                        }
                        let item_name = parts[1..].join(" ");
                        match session.with_inventory(|ctx| break_item(ctx, &item_name)) {
                            Ok(msg) => {
                                println!("{msg}");
                                if let Err(e) = persist_session(&mut session, &persistence).await {
                                    error!(error = %e, "persist after break failed");
                                }
                            }
                            Err(e) => println!("{e}"),
                        }
                    }
                    "harvest" | "gather" => {
                        if parts.len() < 2 {
                            println!("Usage: harvest <object>");
                            continue;
                        }
                        let item_name = parts[1..].join(" ");
                        match session.with_inventory(|ctx| harvest_item(ctx, &item_name)) {
                            Ok(msg) => {
                                println!("{msg}");
                                if let Err(e) = persist_session(&mut session, &persistence).await {
                                    error!(error = %e, "persist after harvest failed");
                                }
                            }
                            Err(e) => println!("{e}"),
                        }
                    }
                    "use" | "drink" | "apply" => {
                        if parts.len() < 2 {
                            println!("Usage: use <item>");
                            continue;
                        }
                        let item_name = parts[1..].join(" ");
                        match session.with_inventory(|ctx| use_item(ctx, &item_name)) {
                            Ok(msg) => {
                                println!("{msg}");
                                if let Err(e) = persist_session(&mut session, &persistence).await {
                                    error!(error = %e, "persist after use failed");
                                }
                            }
                            Err(e) => println!("{e}"),
                        }
                    }
                    "lock" => {
                        if parts.len() < 2 {
                            println!("Usage: lock <container>");
                            continue;
                        }
                        let container_name = parts[1..].join(" ");
                        match session.with_inventory(|ctx| lock_container(ctx, &container_name)) {
                            Ok(msg) => {
                                println!("{msg}");
                                if let Err(e) = persist_session(&mut session, &persistence).await {
                                    error!(error = %e, "persist after lock failed");
                                }
                            }
                            Err(e) => println!("{e}"),
                        }
                    }
                    "unlock" => {
                        let rest = parts[1..].join(" ");
                        match parse_unlock_args(&rest) {
                            Ok((container, key)) => {
                                match session.with_inventory(|ctx| {
                                    unlock_container(ctx, &container, key.as_deref())
                                }) {
                                    Ok(msg) => {
                                        println!("{msg}");
                                        if let Err(e) =
                                            persist_session(&mut session, &persistence).await
                                        {
                                            error!(error = %e, "persist after unlock failed");
                                        }
                                    }
                                    Err(e) => println!("{e}"),
                                }
                            }
                            Err(e) => println!("{e}"),
                        }
                    }
                    "@keyfor" => {
                        if parts.len() < 2 {
                            println!("Usage: @keyfor <container> [key name]");
                            continue;
                        }
                        let container_name = parts[1].to_string();
                        let key_name = if parts.len() > 2 {
                            parts[2..].join(" ")
                        } else {
                            format!("{} Key", container_name)
                        };
                        let container_id = match resolve_container_target(
                            &container_name,
                            session.player_id(),
                            session.current_location(),
                            &session.objects(),
                        ) {
                            Some(id) => id,
                            None => {
                                println!("{}", narrate_wizard_not_found());
                                continue;
                            }
                        };
                        let mut container = session
                            .object(&container_id)
                            .expect("resolved container");
                        let location = session.current_location().cloned();
                        match create_key_for_container(
                            &factory,
                            &mut container,
                            &key_name,
                            player_id.clone(),
                            location,
                        )
                        .await
                        {
                            Ok(key) => {
                                session.upsert_object(container);
                                session.upsert_object(key.clone());
                                println!(
                                    "You conjure {} (lock_id: {}).",
                                    key.name,
                                    key.key_lock_id().unwrap_or_default()
                                );
                            }
                            Err(e) => {
                                error!(error = %e, "@keyfor failed");
                                println!("The key refuses to take shape.");
                            }
                        }
                    }
                    "wield" => {
                        if parts.len() < 2 {
                            println!("Usage: wield <item>");
                            continue;
                        }
                        let item_name = parts[1..].join(" ");
                        match session.with_inventory(|ctx| wield_item(ctx, &item_name)) {
                            Ok(msg) => {
                                println!("{msg}");
                                if let Err(e) = persist_session(&mut session, &persistence).await {
                                    error!(error = %e, "persist after wield failed");
                                }
                            }
                            Err(e) => println!("{e}"),
                        }
                    }
                    "module" => {
                        if parts.len() < 2 {
                            println!("Usage: module reload | module bundle <outdir>");
                            continue;
                        }
                        match parts[1] {
                            "reload" => {
                                let path = default_module_dir();
                                match reload_universe(path.to_str().unwrap_or("modules/default")) {
                                    Ok(universe) => {
                                        loaded_universe = universe;
                                        session.set_anatomy(
                                            loaded_universe.active_world()?.anatomy.clone(),
                                        );
                                        let world = loaded_universe.active_world()?;
                                        info!(
                                            universe = %loaded_universe.name,
                                            world = %world.name,
                                            sources = world.sources.len(),
                                            "module reloaded"
                                        );
                                        println!(
                                            "{}",
                                            narrate_module_reloaded(
                                                &loaded_universe.name,
                                                &world.name,
                                            )
                                        );
                                    }
                                    Err(e) => {
                                        error!(error = %e, "module reload failed");
                                        println!("Reality refuses to reload.");
                                    }
                                }
                            }
                            "bundle" => {
                                if parts.len() < 3 {
                                    println!("Usage: module bundle <output_dir>");
                                    continue;
                                }
                                let out = parts[2];
                                let module_path = default_module_dir();
                                match package_module(
                                    module_path.to_str().unwrap_or("modules/default"),
                                    out,
                                ) {
                                    Ok(manifest) => {
                                        let module_path =
                                            module_path.to_str().unwrap_or("modules/default");
                                        info!(
                                            module = %manifest.name,
                                            output = %out,
                                            files = manifest.files.len(),
                                            "module bundled"
                                        );
                                        println!(
                                            "{}",
                                            narrate_module_bundled(
                                                module_path,
                                                out,
                                                manifest.files.len(),
                                            )
                                        );
                                    }
                                    Err(e) => {
                                        error!(error = %e, "module bundle failed");
                                        println!("The bundle will not hold together.");
                                    }
                                }
                            }
                            other => println!("Unknown module command: {other}"),
                        }
                    }
                    "wear" => {
                        if parts.len() < 2 {
                            println!("Usage: wear <item>");
                            continue;
                        }
                        let item_name = parts[1..].join(" ");
                        match session.with_inventory(|ctx| wear_item(ctx, &item_name)) {
                            Ok(msg) => {
                                println!("{msg}");
                                if let Err(e) = persist_session(&mut session, &persistence).await {
                                    error!(error = %e, "persist after wear failed");
                                }
                            }
                            Err(e) => println!("{e}"),
                        }
                    }

                    "@dig" => {
                        let dig_cmd = match parse_dig_command(input) {
                            Ok(cmd) => cmd,
                            Err(e) => {
                                println!("{e}");
                                continue;
                            }
                        };
                        match session
                            .dig_place(
                                &factory,
                                DigRequest {
                                    direction: dig_cmd.direction,
                                    name: dig_cmd.name,
                                    options: dig_cmd.options,
                                },
                            )
                            .await
                        {
                            Ok(result) => {
                                println!("{}", narrate_dig(&result.new_place, &result.notes));
                                if let Err(e) = persist_session(&mut session, &persistence).await {
                                    error!(error = %e, "persist after @dig failed");
                                }
                            }
                            Err(e) => println!("{e}"),
                        }
                    }
                    "@link" => {
                        let link_cmd = match parse_link_command(input) {
                            Ok(cmd) => cmd,
                            Err(e) => {
                                println!("{e}");
                                continue;
                            }
                        };
                        let from_resolution = if let Some(from_name) = link_cmd.from.as_deref() {
                            session.resolve_target(from_name, ResolveScope::General)
                        } else if let Some(loc) = session.current_location() {
                            TargetResolution::Found(loc.clone())
                        } else {
                            TargetResolution::NotFound
                        };
                        let target_resolution =
                            session.resolve_target(&link_cmd.target, ResolveScope::General);

                        match (from_resolution, target_resolution) {
                            (
                                TargetResolution::Found(from_id),
                                TargetResolution::Found(target_id),
                            ) => {
                                match session.link_exit(
                                    &from_id,
                                    &link_cmd.direction,
                                    &target_id,
                                    link_cmd.reciprocal,
                                    link_cmd.return_exit.as_deref(),
                                ) {
                                    Ok(notes) => {
                                        println!("{}", narrate_link(&notes));
                                        if let Err(e) =
                                            persist_session(&mut session, &persistence).await
                                        {
                                            error!(error = %e, "persist after @link failed");
                                        }
                                    }
                                    Err(e) => println!("{e}"),
                                }
                            }
                            (TargetResolution::Ambiguous(msg), _)
                            | (_, TargetResolution::Ambiguous(msg)) => println!("{msg}"),
                            (TargetResolution::NotFound, _) => println!(
                                "{}",
                                narrate_no_location_builder(
                                    "Set a current location or specify @link <from> <dir> <target>."
                                )
                            ),
                            (_, TargetResolution::NotFound) => {
                                println!("{}", narrate_target_not_found(&link_cmd.target))
                            }
                        }
                    }
                    "@unlink" => {
                        let unlink_cmd = match parse_unlink_command(input) {
                            Ok(cmd) => cmd,
                            Err(e) => {
                                println!("{e}");
                                continue;
                            }
                        };
                        let from_resolution = if let Some(from_name) = unlink_cmd.from.as_deref() {
                            session.resolve_target(from_name, ResolveScope::General)
                        } else if let Some(loc) = session.current_location() {
                            TargetResolution::Found(loc.clone())
                        } else {
                            TargetResolution::NotFound
                        };
                        match from_resolution {
                            TargetResolution::Found(from_id) => {
                                match session.unlink_exit(&from_id, &unlink_cmd.direction) {
                                    Ok(msg) => {
                                        println!("{msg}");
                                        if let Err(e) =
                                            persist_session(&mut session, &persistence).await
                                        {
                                            error!(error = %e, "persist after @unlink failed");
                                        }
                                    }
                                    Err(e) => println!("{e}"),
                                }
                            }
                            TargetResolution::Ambiguous(msg) => println!("{msg}"),
                            TargetResolution::NotFound => println!(
                                "{}",
                                narrate_no_location_builder(
                                    "Set a current location or specify @unlink <from> <dir>."
                                )
                            ),
                        }
                    }
                    "@set" => {
                        let set_cmd = match parse_set_command(&parsed.args) {
                            Ok(cmd) => cmd,
                            Err(e) => {
                                println!("{e}");
                                continue;
                            }
                        };
                        match resolve_in_session(&mut session, &persistence, Some(&set_cmd.target))
                            .await
                        {
                            Ok(TargetResolution::Found(id)) => {
                                let mut obj = match session.object(&id) {
                                    Some(obj) => obj,
                                    None => {
                                        println!("{}", narrate_wizard_not_found());
                                        continue;
                                    }
                                };
                                let graph = session.objects();
                                match apply_set(
                                    &mut obj,
                                    &set_cmd.key,
                                    &set_cmd.value,
                                    session.player_id(),
                                    &graph,
                                ) {
                                    Ok(()) => {
                                        info!(
                                            target = %id,
                                            key = %set_cmd.key,
                                            "wizard @set applied"
                                        );
                                        match session.persist_object(&persistence, obj).await {
                                            Ok(saved) => {
                                                println!(
                                                    "{}",
                                                    narrate_field_set(&saved, &set_cmd.key)
                                                );
                                            }
                                            Err(e) => {
                                                error!(error = %e, "@set save failed");
                                                println!(
                                                    "The change fades before it can take hold."
                                                );
                                            }
                                        }
                                    }
                                    Err(e) => println!("{e}"),
                                }
                            }
                            Ok(TargetResolution::Ambiguous(msg)) => println!("{msg}"),
                            Ok(TargetResolution::NotFound) => {
                                println!("{}", narrate_target_not_found(&set_cmd.target));
                            }
                            Err(e) => {
                                error!(error = %e, "@set failed");
                                println!("You cannot reach that object to change it.");
                            }
                        }
                    }
                    "@unset" => {
                        let unset_cmd = match parse_unset_command(&parsed.args) {
                            Ok(cmd) => cmd,
                            Err(e) => {
                                println!("{e}");
                                continue;
                            }
                        };
                        match resolve_in_session(
                            &mut session,
                            &persistence,
                            Some(&unset_cmd.target),
                        )
                        .await
                        {
                            Ok(TargetResolution::Found(id)) => {
                                let mut obj = match session.object(&id) {
                                    Some(obj) => obj,
                                    None => match persistence.load_object(&id).await {
                                        Ok(Some(o)) => o,
                                        Ok(None) => {
                                            println!("{}", narrate_wizard_not_found());
                                            continue;
                                        }
                                        Err(e) => {
                                            error!(error = %e, id = %id, "@unset load failed");
                                            println!("You cannot reach that object to change it.");
                                            continue;
                                        }
                                    },
                                };
                                match apply_unset(&mut obj, &unset_cmd.key) {
                                    Ok(()) => {
                                        info!(
                                            target = %id,
                                            key = %unset_cmd.key,
                                            "wizard @unset applied"
                                        );
                                        match session.persist_object(&persistence, obj).await {
                                            Ok(saved) => {
                                                println!(
                                                    "{}",
                                                    narrate_field_unset(&saved, &unset_cmd.key)
                                                );
                                            }
                                            Err(e) => {
                                                error!(error = %e, "@unset save failed");
                                                println!(
                                                    "The change fades before it can take hold."
                                                );
                                            }
                                        }
                                    }
                                    Err(e) => println!("{e}"),
                                }
                            }
                            Ok(TargetResolution::Ambiguous(msg)) => println!("{msg}"),
                            Ok(TargetResolution::NotFound) => {
                                println!("{}", narrate_target_not_found(&unset_cmd.target));
                            }
                            Err(e) => {
                                error!(error = %e, "@unset failed");
                                println!("You cannot reach that object to change it.");
                            }
                        }
                    }
                    "load" => {
                        if parts.len() < 2 {
                            println!("Usage: load <id>");
                            continue;
                        }
                        let id = ObjectId::new(parts[1]);
                        match persistence.load_object(&id).await {
                            Ok(Some(obj)) => {
                                info!(id = %id, name = %obj.name, "object loaded into session");
                                session.cache_object(obj.clone());
                                println!("{}", narrate_loaded(&obj.name));
                                let ctx = session.display_context(DisplayMode::Builder);
                                render_object(&obj, &ctx, true, false);
                            }
                            Ok(None) => println!("{}", narrate_wizard_not_found()),
                            Err(e) => {
                                error!(error = %e, id = %id, "load failed");
                                println!("That memory refuses to surface.");
                            }
                        }
                    }
                    "save" => {
                        if parts.len() < 2 {
                            println!("Usage: save <id>");
                            continue;
                        }
                        let id = ObjectId::new(parts[1]);
                        if let Some(obj) = session.object(&id) {
                            let name = obj.name.clone();
                            match session.persist_object(&persistence, obj).await {
                                Ok(_) => {
                                    info!(id = %id, name = %name, "object saved");
                                    println!("{}", narrate_saved(&name));
                                }
                                Err(e) => {
                                    error!(error = %e, id = %id, "save failed");
                                    println!("The archive rejects your commit.");
                                }
                            }
                        } else {
                            println!("{}", narrate_not_in_cache());
                        }
                    }
                    "exit" | "quit" => {
                        println!("Goodbye!");
                        break;
                    }
                    _ => {
                        println!("Unknown command: {}. Type 'help'.", cmd);
                    }
                }
            }
            Err(ReadlineError::Interrupted) => {
                println!("CTRL-C");
                break;
            }
            Err(ReadlineError::Eof) => {
                println!("CTRL-D");
                break;
            }
            Err(err) => {
                println!("Error: {:?}", err);
                break;
            }
        }
    }

    let _ = rl.save_history(&history_path);
    Ok(())
}
