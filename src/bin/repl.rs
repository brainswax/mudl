use std::collections::HashMap;

use anyhow::Result;
use rustyline::{error::ReadlineError, DefaultEditor};

use mudl::command::{
    apply_set, apply_unset, bootstrap_active_universe, create_at_location_with_options,
    create_key_for_container, has_wizard_permission, package_module, parse_command_line,
    parse_create_command, parse_set_command, parse_unset_command, reload_universe,
    resolve_container_target, soft_delete_object, undelete_object, wizard_access_denied,
};
use mudl::display::{
    format_examine_output, format_no_parent_message, narrate_create, narrate_field_set,
    narrate_field_unset, narrate_loaded, narrate_module_bundled, narrate_module_reloaded,
    narrate_no_location, narrate_no_location_builder,
    narrate_not_in_cache, narrate_saved, narrate_target_not_found, narrate_wizard_not_found,
    parse_examine_request, resolve_examine_request, resolve_target, Describable,
    DisplayContext, DisplayFlags, DisplayMode, ExamineError, ExamineResolution, ResolveScope,
    TargetResolution,
};
use mudl::inventory::{
    close_container, describe_inventory, drop_item, lock_container, open_container,
    parse_put_args, parse_unlock_args, put_item, read_item, remove_item, take_item,
    unlock_container, wear_item, wield_item,
};
use mudl::mudl::{default_module_dir, LoadedUniverse};
use mudl::object::{Object, ObjectFactory, ObjectId};
use mudl::persistence::{Persistence, SqlitePersistence};
use mudl::repl::Session;
use mudl::world::movement_direction_from_line;
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
    match resolve_in_session(session, persistence, target).await {
        Ok(TargetResolution::Found(id)) => {
            let mode = if builder {
                DisplayMode::Builder
            } else {
                DisplayMode::Player
            };
            let mut ctx = session.display_context(mode);
            if !builder {
                ctx = ctx.with_flags(DisplayFlags::BRIEF);
            }
            if let Some(obj) = ctx.objects.get(&id) {
                render_object(obj, &ctx, builder, false);
            } else if let Some(target) = target {
                println!("{}", narrate_target_not_found(target));
            } else {
                println!(
                    "{}",
                    if builder {
                        narrate_no_location_builder("Try '@look <target>' or '@look here'.")
                    } else {
                        narrate_no_location()
                    }
                );
            }
        }
        Ok(TargetResolution::Ambiguous(msg)) => println!("{msg}"),
        Ok(TargetResolution::NotFound) => {
            if target.is_some() {
                println!("{}", narrate_target_not_found(target.unwrap()));
            } else {
                println!(
                    "{}",
                    if builder {
                        narrate_no_location_builder("Try '@look <target>' or '@look here'.")
                    } else {
                        narrate_no_location()
                    }
                );
            }
        }
        Err(e) => {
            error!(error = %e, "look failed");
            println!(
                "{}",
                if builder {
                    "The builder view remains obscured."
                } else {
                    "Something stirs in the void, but you cannot make sense of it."
                }
            );
        }
    }
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
        session.anatomy(),
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
                    println!("{}", narrate_target_not_found(args.first().unwrap_or(&"target")));
                }
            }
        }
        Err(ExamineError::Ambiguous(msg)) => println!("{msg}"),
        Err(ExamineError::NoParent(id)) => {
            if let Some(obj) = ctx.objects.get(&id) {
                println!("{}", format_no_parent_message(obj));
            } else {
                println!("{}", narrate_target_not_found(args.first().unwrap_or(&"target")));
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
    dotenv::dotenv().ok();
    init_tracing();

    let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "repl.db".to_string());

    info!("MUDL REPL starting");
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
                if parsed.is_meta && !has_wizard_permission(session.player_id()) {
                    println!("{}", wizard_access_denied());
                    continue;
                }

                let parts: Vec<&str> = input.split_whitespace().collect();
                let cmd = parts[0];

                if cmd == "go" && parts.len() < 2 {
                    println!("Usage: go <direction>  (or just: north, south, in, …)");
                    continue;
                }
                if let Some(dir) = movement_direction_from_line(cmd, &parts[1..]) {
                    match session.go(dir) {
                        Ok(msg) => {
                            println!("{msg}");
                            if let Err(e) = persist_session(&mut session, &persistence).await
                            {
                                error!(error = %e, "persist after go failed");
                            }
                        }
                        Err(e) => println!("{e}"),
                    }
                    continue;
                }

                match cmd {
                    "help" => {
                        println!("Commands:");
                        println!("  create <type> <name...>     - e.g. create sword Rusty Sword");
                        println!("  list                        - list objects in session memory");
                        println!(
                            "  look [target]  (l)          - in-character brief view"
                        );
                        println!(
                            "  @look [target]              - wizard: structured builder view"
                        );
                        println!(
                            "  examine [target]  (x)       - in-character detail (self, .body)"
                        );
                        println!(
                            "  @examine [target] [parent]  - wizard: properties, anatomy, prototype"
                        );
                        println!("  @dump [target]              - wizard: full JSON dump of an object");
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
                        println!("  read <object>               - read text on a note, sign, or mailbox");
                        println!(
                            "  open/close <container|door|window> - open or close a container or portal"
                        );
                        println!(
                            "  lock/unlock <container|door|window> [with <key>] - lock or unlock (auto-finds key)"
                        );
                        println!("  wear <item>                 - wear a container or garment");
                        println!("  go <dir>  (or n/s/e/w/…)    - move; shows room description and exits");
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
                            "  @undelete <id>              - wizard: restore soft-deleted object"
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
                            session.anatomy(),
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
                                let loc = obj
                                    .location
                                    .as_ref()
                                    .and_then(|id| location_object(id, session.objects()));
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
                            println!("Something stirs in the void, but you cannot make sense of it.");
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
                        match resolve_in_session(&mut session, &persistence, target).await
                        {
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
                                    println!(
                                        "{}",
                                        narrate_target_not_found(parts.get(1).unwrap())
                                    );
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
                            session.objects(),
                        ) {
                            Some(id) => {
                                match soft_delete_object(&persistence, &id, session.objects_mut())
                                    .await
                                {
                                    Ok(msg) => println!("{msg}"),
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
                        match undelete_object(&persistence, &id, session.objects_mut()).await {
                            Ok(msg) => println!("{msg}"),
                            Err(e) => {
                                error!(error = %e, id = %id, "undelete failed");
                                println!("Restoration fails — the threads won't reweave.");
                            }
                        }
                    }
                    "inventory" | "i" => {
                        if let Some(player) = session.object(session.player_id()) {
                            println!(
                                "{}",
                                describe_inventory(player, session.objects(), session.anatomy())
                            );
                        } else {
                            println!("You seem to have lost yourself.");
                        }
                    }
                    "get" | "take" => {
                        if parts.len() < 2 {
                            println!("Usage: get <item>");
                            continue;
                        }
                        let item_name = parts[1..].join(" ");
                        let mut ctx = session.inventory_context();
                        match take_item(&mut ctx, &item_name) {
                            Ok(msg) => {
                                println!("{msg}");
                                if let Err(e) = persist_session(&mut session, &persistence).await {
                                    error!(error = %e, "persist after take failed");
                                }
                            }
                            Err(e) => println!("{e}"),
                        }
                    }
                    "drop" => {
                        if parts.len() < 2 {
                            println!("Usage: drop [count] <item>");
                            continue;
                        }
                        let item_name = parts[1..].join(" ");
                        let mut ctx = session.inventory_context();
                        match drop_item(&mut ctx, &item_name) {
                            Ok(msg) => {
                                println!("{msg}");
                                if let Err(e) = persist_session(&mut session, &persistence).await {
                                    error!(error = %e, "persist after drop failed");
                                }
                            }
                            Err(e) => println!("{e}"),
                        }
                    }
                    "put" => {
                        let rest = parts[1..].join(" ");
                        match parse_put_args(&rest) {
                            Ok(req) => {
                                let mut ctx = session.inventory_context();
                                match put_item(
                                    &mut ctx,
                                    &req.item_name,
                                    &req.container_name,
                                    req.quantity,
                                ) {
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
                            let mut ctx = session.inventory_context();
                            match remove_item(&mut ctx, item.trim(), container.trim()) {
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
                        let mut ctx = session.inventory_context();
                        match open_container(&mut ctx, &container_name) {
                            Ok(msg) => {
                                println!("{msg}");
                                if let Err(e) = persist_session(&mut session, &persistence).await
                                {
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
                        let mut ctx = session.inventory_context();
                        match close_container(&mut ctx, &container_name) {
                            Ok(msg) => {
                                println!("{msg}");
                                if let Err(e) = persist_session(&mut session, &persistence).await
                                {
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
                        let ctx = session.inventory_context();
                        match read_item(&ctx, &item_name) {
                            Ok(msg) => println!("{msg}"),
                            Err(e) => println!("{e}"),
                        }
                    }
                    "lock" => {
                        if parts.len() < 2 {
                            println!("Usage: lock <container>");
                            continue;
                        }
                        let container_name = parts[1..].join(" ");
                        let mut ctx = session.inventory_context();
                        match lock_container(&mut ctx, &container_name) {
                            Ok(msg) => {
                                println!("{msg}");
                                if let Err(e) = persist_session(&mut session, &persistence).await
                                {
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
                                let mut ctx = session.inventory_context();
                                match unlock_container(&mut ctx, &container, key.as_deref()) {
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
                            session.objects(),
                        ) {
                            Some(id) => id,
                            None => {
                                println!("{}", narrate_wizard_not_found());
                                continue;
                            }
                        };
                        let mut container = session
                            .object(&container_id)
                            .cloned()
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
                        let mut ctx = session.inventory_context();
                        match wield_item(&mut ctx, &item_name) {
                            Ok(msg) => {
                                println!("{msg}");
                                if let Err(e) = persist_session(&mut session, &persistence).await
                                {
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
                        let mut ctx = session.inventory_context();
                        match wear_item(&mut ctx, &item_name) {
                            Ok(msg) => {
                                println!("{msg}");
                                if let Err(e) = persist_session(&mut session, &persistence).await
                                {
                                    error!(error = %e, "persist after wear failed");
                                }
                            }
                            Err(e) => println!("{e}"),
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
                        match resolve_in_session(
                            &mut session,
                            &persistence,
                            Some(&set_cmd.target),
                        )
                        .await
                        {
                            Ok(TargetResolution::Found(id)) => {
                                let mut obj = match session.object(&id).cloned() {
                                    Some(obj) => obj,
                                    None => {
                                        println!("{}", narrate_wizard_not_found());
                                        continue;
                                    }
                                };
                                match apply_set(
                                    &mut obj,
                                    &set_cmd.key,
                                    &set_cmd.value,
                                    session.player_id(),
                                    session.objects(),
                                ) {
                                    Ok(()) => {
                                        info!(
                                            target = %id,
                                            key = %set_cmd.key,
                                            "wizard @set applied"
                                        );
                                        if let Err(e) = persistence.save_object(&obj).await {
                                            error!(error = %e, "@set save failed");
                                            println!("The change fades before it can take hold.");
                                        } else {
                                            println!(
                                                "{}",
                                                narrate_field_set(&obj, &set_cmd.key)
                                            );
                                        }
                                        session.upsert_object(obj);
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
                                let mut obj = match session.object(&id).cloned() {
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
                                        if let Err(e) = persistence.save_object(&obj).await {
                                            error!(error = %e, "@unset save failed");
                                            println!("The change fades before it can take hold.");
                                        } else {
                                            println!(
                                                "{}",
                                                narrate_field_unset(&obj, &unset_cmd.key)
                                            );
                                        }
                                        session.upsert_object(obj);
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
                            match persistence.save_object(obj).await {
                                Ok(()) => {
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
