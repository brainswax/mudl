use std::collections::HashMap;

use anyhow::Result;
use rustyline::{DefaultEditor, error::ReadlineError};

use mudl::core::object::{Object, ObjectFactory, ObjectId, PermissionFlags, Property, Value, Verb};
use mudl::core::persistence::{Persistence, SqlitePersistence};

fn print_object(obj: &Object) {
    println!("=== {} ===", obj.id);
    println!("Name: {}", obj.name);
    if !obj.aliases.is_empty() {
        println!("Aliases: {}", obj.aliases.join(", "));
    }
    println!("Owner: {}", obj.owner);
    if let Some(loc) = &obj.location {
        println!("Location: {}", loc);
    }
    if let Some(proto) = &obj.prototype {
        println!("Prototype: {}", proto);
    }
    println!("Permissions: {:?}", obj.permissions);
    println!("Properties:");
    for (name, prop) in &obj.properties {
        println!("  {} = {:?} (perms: {:?})", name, prop.value, prop.permissions);
    }
    println!("Verbs:");
    for (name, verb) in &obj.verbs {
        println!("  {}: {} (perms: {:?})", name, verb.code, verb.permissions);
    }
    if !obj.event_handlers.is_empty() {
        println!("Event handlers: {}", obj.event_handlers.len());
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
    let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "repl.db".to_string());

    println!("MUDL REPL starting...");
    let persistence = SqlitePersistence::new(&db_url).await?;
    let factory = ObjectFactory::new(persistence.clone());
    let mut cache: HashMap<ObjectId, Object> = HashMap::new();
    let default_owner = ObjectId::new("player:admin-001");

    println!("Using database: {}", db_url);
    println!("Default owner: {}", default_owner);
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

                rl.add_history_entry(line.as_str());

                let parts: Vec<&str> = input.split_whitespace().collect();
                let cmd = parts[0];
                match cmd {
                    "help" => {
                        println!("Commands:");
                        println!("  create <type> <base_name>   - e.g. create room cozy-kitchen");
                        println!("  list                        - list objects in session cache");
                        println!("  look <id>                   - show object details");
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
                        match factory.create(type_name, base_name, default_owner.clone()).await {
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
                    "look" => {
                        if parts.len() < 2 {
                            println!("Usage: look <id>");
                            continue;
                        }
                        let id = ObjectId::new(parts[1]);
                        let obj = if let Some(o) = cache.get(&id) {
                            o.clone()
                        } else {
                            match persistence.load_object(&id).await {
                                Ok(Some(o)) => {
                                    cache.insert(id.clone(), o.clone());
                                    o
                                }
                                Ok(None) => {
                                    println!("Object not found: {}", id);
                                    continue;
                                }
                                Err(e) => {
                                    println!("Error loading: {}", e);
                                    continue;
                                }
                            }
                        };
                        print_object(&obj);
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
                                print_object(&obj);
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
