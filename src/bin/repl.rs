use std::collections::HashMap;

use anyhow::Result;
use rustyline::{error::ReadlineError, DefaultEditor};

use mudl::core::display::{resolve_target, Describable, DisplayContext, DisplayMode};
use mudl::core::object::{Object, ObjectFactory, ObjectId, PermissionFlags, Property, Value, Verb};
use mudl::core::persistence::{Persistence, SqlitePersistence};

async fn load_all_objects(
    persistence: &SqlitePersistence,
    cache: &HashMap<ObjectId, Object>,
) -> Result<HashMap<ObjectId, Object>> {
    let mut objects: HashMap<ObjectId, Object> = HashMap::new();
    for obj in persistence.list_objects().await? {
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
    persistence: &SqlitePersistence,
    cache: &mut HashMap<ObjectId, Object>,
) -> Result<Option<ObjectId>> {
    let objects = load_all_objects(persistence, cache).await?;

    let id = if let Some(name) = target {
        resolve_target(name, current_location.as_ref(), &objects)
    } else {
        current_location.clone()
    };

    if let Some(ref id) = id {
        if !cache.contains_key(id) {
            if let Some(obj) = persistence.load_object(id).await? {
                cache.insert(id.clone(), obj);
            }
        }
    }

    Ok(id)
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

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
    let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "repl.db".to_string());

    println!("MUDL REPL starting...");
    let persistence = SqlitePersistence::new(&db_url).await?;
    let factory = ObjectFactory::new(persistence.clone());
    let mut cache: HashMap<ObjectId, Object> = HashMap::new();
    let default_owner = ObjectId::new(
        std::env::var("DEFAULT_PLAYER").unwrap_or_else(|_| "player:admin-001".to_string()),
    );

    println!("Using database: {}", db_url);
    println!("Default owner: {}", default_owner);
    println!("Type 'help' for commands.");

    println!("Bootstrapping default world if needed...");
    let mut current_location: Option<ObjectId> = None;
    match factory.bootstrap(default_owner.clone()).await {
        Ok(loc_id) => {
            println!("Bootstrap complete. Starting at: {}", loc_id);
            current_location = Some(loc_id);
        }
        Err(e) => {
            println!("Warning: Bootstrap failed: {}", e);
        }
    }

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

                let parts: Vec<&str> = input.split_whitespace().collect();
                let cmd = parts[0];
                match cmd {
                    "help" => {
                        println!("Commands:");
                        println!("  create <type> <base_name>   - e.g. create room cozy-kitchen");
                        println!("  list                        - list objects in session cache");
                        println!(
                            "  look [target]  (l)          - immersive view (current room if no target)"
                        );
                        println!(
                            "  examine [target]  (x)       - builder view with IDs, properties, verbs"
                        );
                        println!("  @dump [target]              - full JSON dump of an object");
                        println!("  go <dir>                    - move to another location (e.g. go north)");
                        println!("  add_prop <id> <name> <value> - add string property");
                        println!("  add_verb <id> <name> <code> - add verb with code");
                        println!("  load <id>                   - load object from persistence");
                        println!("  save <id>                   - save object from cache");
                        println!("  exit                        - quit");
                    }
                    "create" => {
                        if parts.len() < 3 {
                            println!("Usage: create <type> <base_name>");
                            continue;
                        }
                        let type_name = parts[1];
                        let base_name = parts[2];
                        match factory
                            .create(type_name, base_name, default_owner.clone())
                            .await
                        {
                            Ok(obj) => {
                                println!("Created: {} ({})", &obj.name, &obj.id);
                                cache.insert(obj.id.clone(), obj);
                            }
                            Err(e) => println!("Error creating: {}", e),
                        }
                    }
                    "list" => {
                        if cache.is_empty() {
                            println!("No objects in cache. Use 'load' or 'create'.");
                        } else {
                            println!("Cached objects:");
                            for (id, obj) in &cache {
                                println!("  {} - {}", id, obj.name);
                            }
                        }
                    }
                    "look" | "l" => {
                        let target = parts.get(1).copied();
                        match resolve_and_load(target, &current_location, &persistence, &mut cache)
                            .await
                        {
                            Ok(Some(id)) => {
                                let objects = load_all_objects(&persistence, &cache).await?;
                                let ctx =
                                    DisplayContext::new(default_owner.clone(), DisplayMode::Player)
                                        .with_objects(objects);
                                if let Some(obj) = cache.get(&id) {
                                    render_object(obj, &ctx, false, false);
                                } else {
                                    println!("Object not found: {}", id);
                                }
                            }
                            Ok(None) => {
                                println!(
                                    "No current location. Use 'look <target>' or 'look here'."
                                );
                            }
                            Err(e) => println!("Error: {}", e),
                        }
                    }
                    "examine" | "x" => {
                        let target = parts.get(1).copied();
                        match resolve_and_load(target, &current_location, &persistence, &mut cache)
                            .await
                        {
                            Ok(Some(id)) => {
                                let objects = load_all_objects(&persistence, &cache).await?;
                                let ctx = DisplayContext::new(
                                    default_owner.clone(),
                                    DisplayMode::Builder,
                                )
                                .with_objects(objects);
                                if let Some(obj) = cache.get(&id) {
                                    render_object(obj, &ctx, true, false);
                                } else {
                                    println!("Object not found: {}", id);
                                }
                            }
                            Ok(None) => {
                                println!(
                                    "No current location. Use 'examine <target>' or 'examine here'."
                                );
                            }
                            Err(e) => println!("Error: {}", e),
                        }
                    }
                    "@dump" => {
                        let target = parts.get(1).copied();
                        match resolve_and_load(target, &current_location, &persistence, &mut cache)
                            .await
                        {
                            Ok(Some(id)) => {
                                if let Some(obj) = cache.get(&id) {
                                    println!("{}", obj.dump());
                                } else {
                                    println!("Object not found: {}", id);
                                }
                            }
                            Ok(None) => {
                                println!("No current location. Use '@dump <target>'.");
                            }
                            Err(e) => println!("Error: {}", e),
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
                                        println!("Current location not found.");
                                        continue;
                                    }
                                    Err(e) => {
                                        println!("Error loading location: {}", e);
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
                                            println!("Player not found.");
                                            continue;
                                        }
                                        Err(e) => {
                                            println!("Error loading player: {}", e);
                                            continue;
                                        }
                                    }
                                };
                                player.location = Some(target_id.clone());
                                if let Err(e) = persistence.save_object(&player).await {
                                    println!("Error saving player location: {}", e);
                                } else {
                                    println!("You go {}.", dir);
                                    current_location = Some(target_id.clone());
                                }
                                cache.insert(default_owner.clone(), player);
                                if let Ok(Some(new_loc)) = persistence.load_object(target_id).await
                                {
                                    cache.insert(target_id.clone(), new_loc);
                                }
                            } else {
                                println!("There is no exit {}.", dir);
                            }
                        } else {
                            println!("No current location set. Use 'look' or bootstrap.");
                        }
                    }
                    "add_prop" => {
                        if parts.len() < 4 {
                            println!("Usage: add_prop <id> <name> <value>");
                            continue;
                        }
                        let id = ObjectId::new(parts[1]);
                        let prop_name = parts[2].to_string();
                        let value_str = parts[3..].join(" ");
                        let mut obj = if let Some(o) = cache.remove(&id) {
                            o
                        } else {
                            match persistence.load_object(&id).await {
                                Ok(Some(o)) => o,
                                Ok(None) => {
                                    println!("Object not found: {}", id);
                                    continue;
                                }
                                Err(e) => {
                                    println!("Error: {}", e);
                                    continue;
                                }
                            }
                        };
                        let prop = Property {
                            name: prop_name,
                            value: Value::String(value_str),
                            permissions: PermissionFlags::OWNER,
                            behavior: None,
                        };
                        obj.add_property(prop);
                        if let Err(e) = persistence.save_object(&obj).await {
                            println!("Error saving: {}", e);
                        } else {
                            println!("Property added.");
                        }
                        cache.insert(id, obj);
                    }
                    "add_verb" => {
                        if parts.len() < 4 {
                            println!("Usage: add_verb <id> <name> <code...>");
                            continue;
                        }
                        let id = ObjectId::new(parts[1]);
                        let verb_name = parts[2].to_string();
                        let code = parts[3..].join(" ");
                        let mut obj = if let Some(o) = cache.remove(&id) {
                            o
                        } else {
                            match persistence.load_object(&id).await {
                                Ok(Some(o)) => o,
                                Ok(None) => {
                                    println!("Object not found: {}", id);
                                    continue;
                                }
                                Err(e) => {
                                    println!("Error: {}", e);
                                    continue;
                                }
                            }
                        };
                        let verb = Verb {
                            name: verb_name,
                            code,
                            permissions: PermissionFlags::OWNER,
                        };
                        obj.add_verb(verb);
                        if let Err(e) = persistence.save_object(&obj).await {
                            println!("Error saving: {}", e);
                        } else {
                            println!("Verb added.");
                        }
                        cache.insert(id, obj);
                    }
                    "load" => {
                        if parts.len() < 2 {
                            println!("Usage: load <id>");
                            continue;
                        }
                        let id = ObjectId::new(parts[1]);
                        match persistence.load_object(&id).await {
                            Ok(Some(obj)) => {
                                cache.insert(id.clone(), obj.clone());
                                println!("Loaded.");
                                let objects = load_all_objects(&persistence, &cache).await?;
                                let ctx = DisplayContext::new(
                                    default_owner.clone(),
                                    DisplayMode::Builder,
                                )
                                .with_objects(objects);
                                render_object(&obj, &ctx, true, false);
                            }
                            Ok(None) => println!("Not found: {}", id),
                            Err(e) => println!("Error: {}", e),
                        }
                    }
                    "save" => {
                        if parts.len() < 2 {
                            println!("Usage: save <id>");
                            continue;
                        }
                        let id = ObjectId::new(parts[1]);
                        if let Some(obj) = cache.get(&id) {
                            match persistence.save_object(obj).await {
                                Ok(()) => println!("Saved {}", id),
                                Err(e) => println!("Error: {}", e),
                            }
                        } else {
                            println!("Object not in cache. Use load first.");
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
