use std::collections::HashMap;

use anyhow::Result;
use rustyline::{error::ReadlineError, DefaultEditor};

use mudl::command::{
    apply_set, apply_unset, bootstrap_active_universe, create_at_location_with_options,
    has_wizard_permission, package_module, parse_command_line, parse_create_command,
    parse_set_command, parse_unset_command, persist_inventory_changes, reload_universe,
    soft_delete_object, take_from_location, undelete_object, wizard_access_denied,
};
use mudl::display::{
    format_examine_output, format_no_parent_message, narrate_create, narrate_field_set,
    narrate_field_unset, narrate_go, narrate_loaded, narrate_module_bundled,
    narrate_module_reloaded, narrate_no_exit, narrate_no_location, narrate_no_location_builder,
    narrate_not_in_cache, narrate_saved, narrate_target_not_found, narrate_wizard_not_found,
    parse_examine_request, resolve_examine_request, resolve_object, resolve_target, Describable,
    DisplayContext, DisplayFlags, DisplayMode, ExamineError, ExamineResolution, ResolveScope,
    TargetResolution,
};
use mudl::inventory::{
    describe_inventory, drop_item, parse_put_args, put_item, remove_item, wear_item, wield_item,
    InventoryContext,
};
use mudl::mudl::{default_module_dir, LoadedUniverse};
use mudl::object::{Object, ObjectFactory, ObjectId};
use mudl::persistence::{Persistence, SqlitePersistence};
use mudl::world::restore_session;
use tracing::{error, info, warn};

async fn load_all_objects(
    persistence: &SqlitePersistence,
    cache: &HashMap<ObjectId, Object>,
) -> Result<HashMap<ObjectId, Object>> {
    let mut objects: HashMap<ObjectId, Object> = HashMap::new();
    for obj in persistence.list_objects(false).await? {
        objects.insert(obj.id.clone(), obj);
    }
    for (id, obj) in cache {
        objects.insert(id.clone(), obj.clone());
    }
    Ok(objects)
}

async fn resolve_and_load(
    target: Option<&str>,
    current_location: &Option<ObjectId>,
    observer: &ObjectId,
    persistence: &SqlitePersistence,
    cache: &mut HashMap<ObjectId, Object>,
) -> Result<TargetResolution> {
    let objects = load_all_objects(persistence, cache).await?;

    let resolution = if let Some(name) = target {
        resolve_object(
            name,
            observer,
            current_location.as_ref(),
            &objects,
            ResolveScope::General,
        )
    } else if let Some(loc) = current_location {
        TargetResolution::Found(loc.clone())
    } else {
        TargetResolution::NotFound
    };

    if let TargetResolution::Found(ref id) = resolution {
        if !cache.contains_key(id) {
            if let Some(obj) = persistence.load_object(id).await? {
                cache.insert(id.clone(), obj);
            }
        }
    }

    Ok(resolution)
}

async fn save_all_objects(
    persistence: &SqlitePersistence,
    objects: &HashMap<ObjectId, Object>,
) -> Result<()> {
    persist_inventory_changes(persistence, objects).await
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
    target: Option<&str>,
    builder: bool,
    current_location: &Option<ObjectId>,
    observer: &ObjectId,
    anatomy: &mudl::mudl::AnatomyRegistry,
    persistence: &SqlitePersistence,
    cache: &mut HashMap<ObjectId, Object>,
) -> Result<(), anyhow::Error> {
    match resolve_and_load(target, current_location, observer, persistence, cache).await {
        Ok(TargetResolution::Found(id)) => {
            let objects = load_all_objects(persistence, cache).await?;
            let mode = if builder {
                DisplayMode::Builder
            } else {
                DisplayMode::Player
            };
            let mut ctx = DisplayContext::new(observer.clone(), mode)
                .with_objects(objects)
                .with_anatomy(anatomy.clone());
            if !builder {
                ctx = ctx.with_flags(DisplayFlags::BRIEF);
            }
            if let Some(obj) = cache.get(&id) {
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
    args: &[&str],
    mode: DisplayMode,
    current_location: &Option<ObjectId>,
    observer: &ObjectId,
    anatomy: &mudl::mudl::AnatomyRegistry,
    persistence: &SqlitePersistence,
    cache: &mut HashMap<ObjectId, Object>,
    builder: bool,
) -> Result<(), anyhow::Error> {
    let request = parse_examine_request(args);
    let objects = load_all_objects(persistence, cache).await?;
    let ctx = DisplayContext::new(observer.clone(), mode)
        .with_objects(objects)
        .with_anatomy(anatomy.clone());

    match resolve_examine_request(&request, anatomy, observer, current_location.as_ref(), &ctx.objects)
    {
        Ok(resolution) => {
            if let ExamineResolution::Prototype { prototype_id, .. } = &resolution {
                if !cache.contains_key(prototype_id) {
                    if let Some(proto) = persistence.load_object(prototype_id).await? {
                        cache.insert(prototype_id.clone(), proto);
                    }
                }
            }
            let objects = load_all_objects(persistence, cache).await?;
            let ctx = ctx.with_objects(objects);
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
    let mut cache: HashMap<ObjectId, Object> = HashMap::new();
    let default_owner = ObjectId::new(
        std::env::var("DEFAULT_PLAYER").unwrap_or_else(|_| "player:admin-001".to_string()),
    );

    info!(database = %db_url, player = %default_owner, "session configuration");

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

    let mut current_location: Option<ObjectId> = None;
    match bootstrap_active_universe(&factory, default_owner.clone()).await {
        Ok((universe, loc_id)) => {
            loaded_universe = universe;
            active_anatomy = loaded_universe.active_world()?.anatomy.clone();
            current_location = Some(loc_id.clone());
            info!(location = %loc_id, "world bootstrapped");
        }
        Err(e) => {
            warn!(error = %e, "bootstrap failed");
        }
    }

    match restore_session(
        &persistence,
        default_owner.clone(),
        current_location.clone(),
    )
    .await
    {
        Ok(session) => {
            cache = session.objects;
            current_location = session.current_location;
            info!(
                objects = cache.len(),
                location = ?current_location,
                "session restored"
            );
        }
        Err(e) => {
            warn!(error = %e, "failed to restore session");
        }
    }

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
                if parsed.is_meta && !has_wizard_permission(&default_owner) {
                    println!("{}", wizard_access_denied());
                    continue;
                }

                let parts: Vec<&str> = input.split_whitespace().collect();
                let cmd = parts[0];
                match cmd {
                    "help" => {
                        println!("Commands:");
                        println!("  create <type> <name...>     - e.g. create sword Rusty Sword");
                        println!("  list                        - list objects in session cache");
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
                            "  put [count] <item> in <container> - stow items (e.g. put 10 coins in purse)"
                        );
                        println!(
                            "  remove <item> from <container> - take an item out of a container"
                        );
                        println!("  wield <item>                - hold/wield an item in your hand");
                        println!("  wear <item>                 - wear a container or garment");
                        println!("  go <dir>                    - move to another location (e.g. go north)");
                        println!(
                            "  @set <target> <key> <value>  - wizard: set property/state/verb"
                        );
                        println!("  @unset <target> <key>        - wizard: remove property/verb");
                        println!("  load <id>                   - load object from persistence");
                        println!("  save <id>                   - save object from cache");
                        println!("  module reload               - reload MUDL module from disk");
                        println!(
                            "  module bundle <outdir>      - package module to output directory"
                        );
                        println!(
                            "  @create <type> <name...> [key=value...] - wizard create with roles"
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
                            default_owner.clone(),
                            current_location.as_ref(),
                            &active_anatomy,
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
                                let objects = load_all_objects(&persistence, &cache).await?;
                                let loc = obj
                                    .location
                                    .as_ref()
                                    .and_then(|id| location_object(id, &objects));
                                println!("{}", narrate_create(&obj, loc));
                                cache.insert(obj.id.clone(), obj);
                            }
                            Err(e) => {
                                error!(error = %e, "create failed");
                                println!("Your conjuration fizzles.");
                            }
                        }
                    }
                    "list" => {
                        if cache.is_empty() {
                            println!("Your working memory is empty.");
                        } else {
                            let names: Vec<String> =
                                cache.values().map(|obj| obj.name.clone()).collect();
                            println!("You recall: {}", names.join(", "));
                            for (id, obj) in &cache {
                                info!(id = %id, name = %obj.name, "cached object");
                            }
                        }
                    }
                    "look" | "l" => {
                        if let Err(e) = run_look_command(
                            parts.get(1).copied(),
                            false,
                            &current_location,
                            &default_owner,
                            &active_anatomy,
                            &persistence,
                            &mut cache,
                        )
                        .await
                        {
                            error!(error = %e, "look failed");
                            println!("Something stirs in the void, but you cannot make sense of it.");
                        }
                    }
                    "@look" => {
                        if let Err(e) = run_look_command(
                            parts.get(1).copied(),
                            true,
                            &current_location,
                            &default_owner,
                            &active_anatomy,
                            &persistence,
                            &mut cache,
                        )
                        .await
                        {
                            error!(error = %e, "@look failed");
                            println!("The builder view remains obscured.");
                        }
                    }
                    "examine" | "x" => {
                        if let Err(e) = run_examine_command(
                            &parts[1..],
                            DisplayMode::Player,
                            &current_location,
                            &default_owner,
                            &active_anatomy,
                            &persistence,
                            &mut cache,
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
                            &parts[1..],
                            DisplayMode::Builder,
                            &current_location,
                            &default_owner,
                            &active_anatomy,
                            &persistence,
                            &mut cache,
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
                        match resolve_and_load(
                            target,
                            &current_location,
                            &default_owner,
                            &persistence,
                            &mut cache,
                        )
                        .await
                        {
                            Ok(TargetResolution::Found(id)) => {
                                if let Some(obj) = cache.get(&id) {
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
                        let objects = load_all_objects(&persistence, &cache).await?;
                        match resolve_target(
                            &target,
                            current_location.as_ref(),
                            Some(&default_owner),
                            &objects,
                        ) {
                            Some(id) => {
                                match soft_delete_object(&persistence, &id, &mut cache).await {
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
                        match undelete_object(&persistence, &id, &mut cache).await {
                            Ok(msg) => println!("{msg}"),
                            Err(e) => {
                                error!(error = %e, id = %id, "undelete failed");
                                println!("Restoration fails — the threads won't reweave.");
                            }
                        }
                    }
                    "inventory" | "i" => {
                        let objects = load_all_objects(&persistence, &cache).await?;
                        if let Some(player) = objects.get(&default_owner).cloned() {
                            println!("{}", describe_inventory(&player, &objects, &active_anatomy));
                        } else if let Ok(Some(player)) =
                            persistence.load_object(&default_owner).await
                        {
                            println!("{}", describe_inventory(&player, &objects, &active_anatomy));
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
                        let mut objects = load_all_objects(&persistence, &cache).await?;
                        match take_from_location(
                            &default_owner,
                            current_location.as_ref(),
                            &item_name,
                            &mut objects,
                            &active_anatomy,
                        ) {
                            Ok(msg) => {
                                println!("{msg}");
                                save_all_objects(&persistence, &objects).await?;
                                cache.extend(objects);
                            }
                            Err(e) => println!("{e}"),
                        }
                    }
                    "drop" => {
                        if parts.len() < 2 {
                            println!("Usage: drop <item>");
                            continue;
                        }
                        let item_name = parts[1..].join(" ");
                        let mut objects = load_all_objects(&persistence, &cache).await?;
                        let mut ctx = InventoryContext {
                            player_id: &default_owner,
                            room_id: current_location.as_ref(),
                            objects: &mut objects,
                            anatomy: &active_anatomy,
                        };
                        match drop_item(&mut ctx, &item_name) {
                            Ok(msg) => {
                                println!("{msg}");
                                save_all_objects(&persistence, &objects).await?;
                                cache.extend(objects);
                            }
                            Err(e) => println!("{e}"),
                        }
                    }
                    "put" => {
                        let rest = parts[1..].join(" ");
                        match parse_put_args(&rest) {
                            Ok(req) => {
                                let mut objects = load_all_objects(&persistence, &cache).await?;
                                let mut ctx = InventoryContext {
                                    player_id: &default_owner,
                                    room_id: current_location.as_ref(),
                                    objects: &mut objects,
                                    anatomy: &active_anatomy,
                                };
                                match put_item(
                                    &mut ctx,
                                    &req.item_name,
                                    &req.container_name,
                                    req.quantity,
                                ) {
                                    Ok(msg) => {
                                        println!("{msg}");
                                        save_all_objects(&persistence, &objects).await?;
                                        cache.extend(objects);
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
                            let mut objects = load_all_objects(&persistence, &cache).await?;
                            let mut ctx = InventoryContext {
                                player_id: &default_owner,
                                room_id: current_location.as_ref(),
                                objects: &mut objects,
                                anatomy: &active_anatomy,
                            };
                            match remove_item(&mut ctx, item.trim(), container.trim()) {
                                Ok(msg) => {
                                    println!("{msg}");
                                    save_all_objects(&persistence, &objects).await?;
                                    cache.extend(objects);
                                }
                                Err(e) => println!("{e}"),
                            }
                        } else {
                            println!("Usage: remove <item> from <container>");
                        }
                    }
                    "wield" => {
                        if parts.len() < 2 {
                            println!("Usage: wield <item>");
                            continue;
                        }
                        let item_name = parts[1..].join(" ");
                        let mut objects = load_all_objects(&persistence, &cache).await?;
                        let mut ctx = InventoryContext {
                            player_id: &default_owner,
                            room_id: current_location.as_ref(),
                            objects: &mut objects,
                            anatomy: &active_anatomy,
                        };
                        match wield_item(&mut ctx, &item_name) {
                            Ok(msg) => {
                                println!("{msg}");
                                save_all_objects(&persistence, &objects).await?;
                                cache.extend(objects);
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
                                        active_anatomy =
                                            loaded_universe.active_world()?.anatomy.clone();
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
                        let mut objects = load_all_objects(&persistence, &cache).await?;
                        let mut ctx = InventoryContext {
                            player_id: &default_owner,
                            room_id: current_location.as_ref(),
                            objects: &mut objects,
                            anatomy: &active_anatomy,
                        };
                        match wear_item(&mut ctx, &item_name) {
                            Ok(msg) => {
                                println!("{msg}");
                                save_all_objects(&persistence, &objects).await?;
                                cache.extend(objects);
                            }
                            Err(e) => println!("{e}"),
                        }
                    }
                    "go" => {
                        if parts.len() < 2 {
                            println!("Usage: go <direction>");
                            continue;
                        }
                        let dir = parts[1];
                        if let Some(loc_id) = &current_location {
                            let loc = if let Some(o) = cache.get(loc_id) {
                                o.clone()
                            } else {
                                match persistence.load_object(loc_id).await {
                                    Ok(Some(o)) => {
                                        cache.insert(loc_id.clone(), o.clone());
                                        o
                                    }
                                    Ok(None) => {
                                        println!(
                                            "The ground shifts beneath you — you are nowhere."
                                        );
                                        continue;
                                    }
                                    Err(e) => {
                                        error!(error = %e, "failed to load location");
                                        println!(
                                            "The ground shifts beneath you — you are nowhere."
                                        );
                                        continue;
                                    }
                                }
                            };
                            let exits = loc.get_exits();
                            if let Some(target_id) = exits.get(dir) {
                                let mut player = if let Some(o) = cache.remove(&default_owner) {
                                    o
                                } else {
                                    match persistence.load_object(&default_owner).await {
                                        Ok(Some(o)) => o,
                                        Ok(None) => {
                                            println!("You seem to have lost yourself.");
                                            continue;
                                        }
                                        Err(e) => {
                                            error!(error = %e, "failed to load player");
                                            println!("You seem to have lost yourself.");
                                            continue;
                                        }
                                    }
                                };
                                player.location = Some(target_id.clone());
                                if let Err(e) = persistence.save_object(&player).await {
                                    error!(error = %e, "failed to save player location");
                                    println!("You try to move, but something holds you in place.");
                                } else {
                                    println!("{}", narrate_go(dir));
                                    current_location = Some(target_id.clone());
                                }
                                cache.insert(default_owner.clone(), player);
                                if let Ok(Some(new_loc)) = persistence.load_object(target_id).await
                                {
                                    cache.insert(target_id.clone(), new_loc);
                                }
                            } else {
                                println!("{}", narrate_no_exit(dir));
                            }
                        } else {
                            println!("{}", narrate_no_location());
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
                        match resolve_and_load(
                            Some(&set_cmd.target),
                            &current_location,
                            &default_owner,
                            &persistence,
                            &mut cache,
                        )
                        .await
                        {
                            Ok(TargetResolution::Found(id)) => {
                                let objects = load_all_objects(&persistence, &cache).await?;
                                let mut obj = match cache.remove(&id).or_else(|| objects.get(&id).cloned()) {
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
                                    &default_owner,
                                    &objects,
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
                                        cache.insert(id, obj);
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
                        match resolve_and_load(
                            Some(&unset_cmd.target),
                            &current_location,
                            &default_owner,
                            &persistence,
                            &mut cache,
                        )
                        .await
                        {
                            Ok(TargetResolution::Found(id)) => {
                                let mut obj = if let Some(o) = cache.remove(&id) {
                                    o
                                } else {
                                    match persistence.load_object(&id).await {
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
                                    }
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
                                        cache.insert(id, obj);
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
                                cache.insert(id.clone(), obj.clone());
                                println!("{}", narrate_loaded(&obj.name));
                                let objects = load_all_objects(&persistence, &cache).await?;
                                let ctx = DisplayContext::new(
                                    default_owner.clone(),
                                    DisplayMode::Builder,
                                )
                                .with_objects(objects)
                                .with_anatomy(active_anatomy.clone());
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
                        if let Some(obj) = cache.get(&id) {
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
