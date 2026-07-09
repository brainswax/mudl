//! Command-layer helpers shared by REPL and future frontends.

pub mod dispatcher;
pub mod editor;
pub mod parse;
pub mod player_input;
pub mod place;
pub mod trigger;

pub use editor::{
    apply_set, apply_unset, parse_set_command, parse_unset_command, ParsedSetCommand,
    ParsedUnsetCommand,
};
pub use dispatcher::{
    CommandDispatcher, CommandResult, LookOptions, MovementChange, PlayerDispatchOptions,
    SocialIntent,
};
pub use parse::{is_meta_command, parse_command_line, CommandLine};
pub use player_input::{
    exit_index_for_current_room, is_known_player_verb, is_logged_out_channel_verb,
    is_open_channel_game_command, is_recognized_player_command,
};
pub use crate::gateway::{
    authorize_meta_command, authorize_plain_command, ActorTier, AuthError,
};
pub use place::{
    parse_dig_command, parse_link_command, parse_unlink_command, ParsedDigCommand,
    ParsedLinkCommand, ParsedUnlinkCommand,
};
pub use trigger::{
    apply_trigger_add, apply_trigger_clear, apply_trigger_remove, apply_trigger_set,
    format_trigger_list, narrate_trigger_added, narrate_trigger_cleared, narrate_trigger_removed,
    narrate_trigger_set, narrate_trigger_test_empty, parse_trigger_command, preview_trigger_test,
    resolve_trigger_target_name, trigger_command_help, validate_trigger_host, TriggerCommand,
    TriggerError,
};

use std::collections::HashMap;

use crate::display::{resolve_object, ResolveScope, TargetResolution};
use crate::inventory::{take_item, InventoryContext, InventoryError};
use crate::world::DispatchStack;
use crate::mudl::{load_module, AnatomyRegistry, LoadedUniverse, MudlRoleProps};
use crate::object::{ContainerSpec, Object, ObjectFactory, ObjectId, WearableSpec};
use crate::persistence::Persistence;
use crate::world::{bootstrap_world, bundle_module, ModuleManifest};

/// Load the active MUDL universe from `MUDL_MODULE` / `MUDL_UNIVERSE` env or default.
pub fn load_active_universe() -> anyhow::Result<LoadedUniverse> {
    crate::mudl::load_module(crate::mudl::default_module_dir())
}

/// Bootstrap the active universe's world for a player.
pub async fn bootstrap_active_universe<P: Persistence>(
    factory: &ObjectFactory<P>,
    owner: ObjectId,
) -> anyhow::Result<(LoadedUniverse, crate::object::ObjectId)> {
    let universe = load_active_universe()?;
    let world = universe.active_world()?;
    let start = bootstrap_world(factory, owner, world).await?;
    Ok((universe, start))
}

/// Package a universe module directory for distribution.
pub fn package_module(module_dir: &str, output_dir: &str) -> anyhow::Result<ModuleManifest> {
    bundle_module(module_dir, output_dir)
}

/// Reload a universe module from disk (for hot-reload during development).
pub fn reload_universe(path: &str) -> anyhow::Result<LoadedUniverse> {
    load_module(path)
}

/// Parsed `create` / `@create` command: clean display name separate from role options.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedCreateCommand {
    pub type_name: String,
    pub display_name: String,
    pub options: CreateOptions,
}

/// Whether a token is a `key=value` option (not part of the display name).
pub fn is_option_token(token: &str) -> bool {
    token.split_once('=').is_some_and(|(key, value)| {
        !key.is_empty()
            && !value.is_empty()
            && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// Parse `create <type> <name...> [key=value...]` or `@create ...`.
///
/// Options (`capacity=3`, `max_weight=10`, etc.) are stripped from the display name.
pub fn parse_create_command(input: &str) -> anyhow::Result<ParsedCreateCommand> {
    let trimmed = input.trim();
    let without_at = trimmed.strip_prefix('@').unwrap_or(trimmed);
    let rest = without_at
        .strip_prefix("create")
        .ok_or_else(|| anyhow::anyhow!("Usage: create <type> <name...> [key=value...]"))?
        .trim_start();

    if rest.is_empty() {
        anyhow::bail!("Usage: create <type> <name...> [key=value...]");
    }

    let type_name = rest
        .split_whitespace()
        .next()
        .ok_or_else(|| anyhow::anyhow!("Usage: create <type> <name...>"))?
        .to_ascii_lowercase();

    let type_end = rest
        .find(&type_name)
        .ok_or_else(|| anyhow::anyhow!("Usage: create <type> <name...>"))?
        + type_name.len();
    let after_type = rest[type_end..].trim_start();

    if after_type.is_empty() {
        anyhow::bail!("Usage: create <type> <name...> [key=value...]");
    }

    let (display_name, option_tokens) = split_display_name_and_options(after_type)?;
    if display_name.is_empty() {
        anyhow::bail!("Usage: create <type> <name...> [key=value...]");
    }

    Ok(ParsedCreateCommand {
        type_name,
        display_name,
        options: parse_create_options(&option_tokens),
    })
}

/// Parse `create <type> <name...>` supporting multi-word and quoted display names.
///
/// Key=value options are stripped; use [`parse_create_command`] when options are needed.
pub fn parse_create_args(parts: &[&str], input: &str) -> anyhow::Result<(String, String)> {
    let _ = parts;
    let parsed = parse_create_command(input)?;
    Ok((parsed.type_name, parsed.display_name))
}

fn split_display_name_and_options(after_type: &str) -> anyhow::Result<(String, Vec<&str>)> {
    if let Some((name, remainder)) = parse_leading_quoted_name(after_type) {
        let opts: Vec<&str> = remainder
            .split_whitespace()
            .filter(|t| is_option_token(t))
            .collect();
        return Ok((name, opts));
    }

    let tokens: Vec<&str> = after_type.split_whitespace().collect();
    let opt_idx = tokens.iter().position(|t| is_option_token(t));

    match opt_idx {
        None => Ok((parse_display_name(after_type)?, Vec::new())),
        Some(idx) => {
            let first_opt = tokens[idx];
            let opt_byte = after_type
                .find(first_opt)
                .ok_or_else(|| anyhow::anyhow!("invalid create command"))?;
            let name_raw = after_type[..opt_byte].trim();
            let display_name = parse_display_name(name_raw)?;
            Ok((display_name, tokens[idx..].to_vec()))
        }
    }
}

fn parse_leading_quoted_name(s: &str) -> Option<(String, &str)> {
    let s = s.trim_start();
    let quote = s.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let inner = &s[1..];
    let end = inner.find(quote)?;
    let name = inner[..end].to_string();
    let remainder = inner[end + 1..].trim();
    Some((name, remainder))
}

fn parse_display_name(raw: &str) -> anyhow::Result<String> {
    let trimmed = raw.trim();
    if trimmed.len() >= 2 {
        if let Some(inner) = trimmed.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
            return Ok(inner.to_string());
        }
        if let Some(inner) = trimmed
            .strip_prefix('\'')
            .and_then(|s| s.strip_suffix('\''))
        {
            return Ok(inner.to_string());
        }
    }
    Ok(trimmed.to_string())
}

/// Optional wizard overrides for `@create` (capacity, weight limits, stack count, etc.).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CreateOptions {
    pub capacity: Option<u32>,
    pub max_weight: Option<i64>,
    pub max_volume: Option<i64>,
    pub weight: Option<f64>,
    pub volume: Option<f64>,
    pub wearable: Option<bool>,
    pub wear_slot: Option<String>,
    pub stack_count: Option<u32>,
    pub lock_id: Option<String>,
    pub locked: Option<bool>,
    pub prototype: Option<ObjectId>,
    pub mudl_props: MudlRoleProps,
}

/// Parse `key=value` tokens from `@create` trailing arguments.
pub fn parse_create_options(tokens: &[&str]) -> CreateOptions {
    let mut opts = CreateOptions::default();
    let mut extra_pairs: Vec<(&str, &str)> = Vec::new();
    for token in tokens {
        if let Some((key, value)) = token.split_once('=') {
            let value = value.trim_matches('"');
            match key {
                "capacity" => opts.capacity = value.parse().ok(),
                "max_weight" | "weight_limit" => opts.max_weight = value.parse().ok(),
                "max_volume" | "volume_limit" => opts.max_volume = value.parse().ok(),
                "weight" => opts.weight = parse_create_number(value),
                "volume" => opts.volume = parse_create_number(value),
                "wearable" => opts.wearable = Some(value == "true"),
                "wear_slot" => opts.wear_slot = Some(value.to_string()),
                "count" | "stack_count" => opts.stack_count = value.parse().ok(),
                "prototype" => opts.prototype = Some(ObjectId::new(value)),
                "lock_id" => opts.lock_id = Some(value.to_string()),
                "locked" | "is_locked" => opts.locked = Some(value == "true"),
                "allowed_types" => opts.mudl_props.allowed_types = Some(value.to_string()),
                _ => extra_pairs.push((key, value)),
            }
        }
    }
    if !extra_pairs.is_empty() {
        opts.mudl_props = MudlRoleProps::from_pairs(&extra_pairs);
    }
    opts
}

/// Parse an integer or decimal for create overrides (`weight=0.1`, `capacity=3`).
pub fn parse_create_number(value: &str) -> Option<f64> {
    let v = value.trim();
    v.parse::<f64>().ok().filter(|n| n.is_finite())
}

/// Apply scalar create overrides (weight, volume, hand_slot, etc.) onto a new object.
pub fn apply_create_property_overrides(obj: &mut Object, options: &CreateOptions) {
    if let Some(w) = options.weight {
        obj.set_property_numeric("weight", w);
    }
    if let Some(v) = options.volume {
        obj.set_property_numeric("volume", v);
    }
    options.mudl_props.apply_scalar_overrides(obj);
}

/// Create an object and place it at the player's current location when one is set.
pub async fn create_at_location<P: Persistence>(
    factory: &ObjectFactory<P>,
    type_name: &str,
    display_name: &str,
    owner: ObjectId,
    location: Option<&ObjectId>,
    anatomy: &AnatomyRegistry,
) -> anyhow::Result<Object> {
    create_at_location_with_options(
        factory,
        type_name,
        display_name,
        owner,
        location,
        anatomy,
        CreateOptions::default(),
    )
    .await
}

/// Create with wizard options (`@create container "Bag" capacity=10`).
pub async fn create_at_location_with_options<P: Persistence>(
    factory: &ObjectFactory<P>,
    type_name: &str,
    display_name: &str,
    owner: ObjectId,
    location: Option<&ObjectId>,
    anatomy: &AnatomyRegistry,
    options: CreateOptions,
) -> anyhow::Result<Object> {
    let mut obj = match type_name {
        "player" => {
            factory
                .create_player(display_name, owner.clone(), anatomy)
                .await?
        }
        "item" => factory.create_item(display_name, owner.clone()).await?,
        "container" => {
            factory
                .create_container_with_spec(
                    display_name,
                    owner.clone(),
                    ContainerSpec {
                        capacity: options.capacity.unwrap_or(10),
                        max_weight: options.max_weight,
                        max_volume: options.max_volume,
                        wearable: options.wearable.unwrap_or(true),
                        wear_slot: options.wear_slot.clone(),
                        lock_id: options.lock_id.clone(),
                        locked: options.locked.unwrap_or(false),
                        allowed_types: options
                            .mudl_props
                            .allowed_types
                            .as_ref()
                            .map(|s| crate::object::parse_allowed_types(s))
                            .filter(|types| !types.is_empty()),
                        ..crate::object::ContainerSpec::default()
                    },
                    options.prototype.clone(),
                )
                .await?
        }
        "key" => {
            let lock_id = options
                .lock_id
                .clone()
                .or_else(|| options.mudl_props.lock_id.clone())
                .ok_or_else(|| anyhow::anyhow!("Usage: @create key <name> lock_id=<lock>"))?;
            factory
                .create_key(
                    display_name,
                    owner.clone(),
                    &lock_id,
                    options.prototype.clone(),
                )
                .await?
        }
        "wearable" => {
            factory
                .create_wearable(
                    display_name,
                    owner.clone(),
                    WearableSpec {
                        wear_slot: options
                            .wear_slot
                            .clone()
                            .unwrap_or_else(|| "torso".to_string()),
                        weight: options
                            .max_weight
                            .map(|w| w as f64)
                            .or(options.weight)
                            .unwrap_or(1.0),
                        volume: options
                            .max_volume
                            .map(|v| v as f64)
                            .or(options.volume)
                            .unwrap_or(1.0),
                        mod_max_weight: None,
                        mod_encumbrance: None,
                        mod_max_health: None,
                        stat_mods: HashMap::new(),
                        skill_mods: HashMap::new(),
                        grant_effects: Vec::new(),
                    },
                    options.prototype.clone(),
                )
                .await?
        }
        "stackable" => {
            factory
                .create_stackable_item(
                    display_name,
                    owner.clone(),
                    options.prototype.clone(),
                    options.stack_count.unwrap_or(1),
                )
                .await?
        }
        other => {
            let slug = crate::object::id_base_from_display_name(display_name);
            let mut obj = factory
                .create_named(other, &slug, display_name, owner)
                .await?;
            if let Some(proto) = &options.prototype {
                obj.prototype = Some(proto.clone());
            }
            if options.mudl_props != MudlRoleProps::default() {
                options.mudl_props.apply_to(&mut obj);
            }
            obj
        }
    };

    if options.weight.is_some()
        || options.volume.is_some()
        || options.mudl_props.has_scalar_overrides()
    {
        apply_create_property_overrides(&mut obj, &options);
        crate::persistence::save_and_sync(factory.persistence(), &mut obj).await?;
    }

    if let Some(loc_id) = location {
        obj.location = Some(loc_id.clone());
        crate::persistence::save_and_sync(factory.persistence(), &mut obj).await?;
    }

    Ok(obj)
}

/// Create a key that opens `container`, assigning a `lock_id` if the container lacks one.
pub async fn create_key_for_container<P: Persistence>(
    factory: &ObjectFactory<P>,
    container: &mut Object,
    key_display_name: &str,
    owner: ObjectId,
    location: Option<ObjectId>,
) -> anyhow::Result<Object> {
    if !container.is_container() {
        anyhow::bail!("{key_display_name}: target is not a container");
    }
    let lock_id = container.ensure_container_lock_id();
    crate::persistence::save_and_sync(factory.persistence(), container).await?;
    let mut key = factory
        .create_key(key_display_name, owner, &lock_id, None)
        .await?;
    if let Some(loc) = location {
        key.location = Some(loc);
        crate::persistence::save_and_sync(factory.persistence(), &mut key).await?;
    }
    Ok(key)
}

/// Resolve a container target for wizard key creation.
pub fn resolve_container_target(
    name: &str,
    player_id: &ObjectId,
    room_id: Option<&ObjectId>,
    objects: &HashMap<ObjectId, Object>,
) -> Option<ObjectId> {
    match resolve_object(
        name,
        player_id,
        room_id,
        objects,
        ResolveScope::PossessionOrRoom,
    ) {
        TargetResolution::Found(id) => objects
            .get(&id)
            .filter(|o| o.is_container())
            .map(|o| o.id.clone()),
        _ => None,
    }
}

/// Pick up a visible item from the current location into the player's hand slots.
pub fn take_from_location(
    player_id: &ObjectId,
    location_id: Option<&ObjectId>,
    item_name: &str,
    objects: &mut HashMap<ObjectId, Object>,
    anatomy: &AnatomyRegistry,
) -> Result<String, InventoryError> {
    let mut dispatch = DispatchStack::default();
    let mut ctx = InventoryContext {
        player_id,
        room_id: location_id,
        objects,
        anatomy,
        dispatch: &mut dispatch,
        dirty: None,
    };
    take_item(&mut ctx, item_name)
}

/// Soft-delete an object by ID (wizard). Object remains in the database.
pub async fn soft_delete_object<P: Persistence>(
    persistence: &P,
    id: &ObjectId,
    objects: &mut HashMap<ObjectId, Object>,
) -> anyhow::Result<String> {
    let mut obj = persistence
        .load_object(id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Object not found: {id}"))?;
    let name = obj.name.clone();
    obj.soft_delete();
    crate::persistence::save_object_with_retry(
        persistence,
        &mut obj,
        crate::persistence::DEFAULT_SAVE_RETRIES,
    )
    .await?;
    objects.insert(id.clone(), obj);
    Ok(crate::display::narrate_soft_delete(&name))
}

/// Restore a soft-deleted object by ID (wizard).
pub async fn undelete_object<P: Persistence>(
    persistence: &P,
    id: &ObjectId,
    objects: &mut HashMap<ObjectId, Object>,
) -> anyhow::Result<String> {
    let mut obj = persistence
        .load_object(id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Object not found: {id}"))?;
    if !obj.is_deleted {
        anyhow::bail!("{id} is not deleted.");
    }
    let name = obj.name.clone();
    obj.undelete();
    crate::persistence::save_object_with_retry(
        persistence,
        &mut obj,
        crate::persistence::DEFAULT_SAVE_RETRIES,
    )
    .await?;
    objects.insert(id.clone(), obj);
    Ok(crate::display::narrate_restore(&name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::editor::{apply_set, apply_unset};
    use crate::display::{narrate_create, Describable, DisplayContext, DisplayMode};
    use crate::inventory::describe_carried;
    use crate::object::{id_base_from_display_name, slugify_display_name};
    use crate::persistence::SqlitePersistence;
    use crate::world::hydrate_world;

    async fn test_factory() -> ObjectFactory<SqlitePersistence> {
        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        ObjectFactory::new(persistence)
    }

    fn test_anatomy() -> AnatomyRegistry {
        load_module("modules/default")
            .unwrap()
            .active_world()
            .unwrap()
            .anatomy
            .clone()
    }

    fn make_area(id: &str, name: &str, desc: &str, owner: ObjectId) -> Object {
        let mut area = Object {
            id: ObjectId::new(id),
            name: name.to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: owner.clone(),
            permissions: crate::object::PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            revision: 0,
            updated_at: None,
            is_deleted: false,
            deleted_at: None,
        };
        area.add_property(crate::object::Property {
            name: "description".to_string(),
            value: crate::object::Value::String(desc.to_string()),
            permissions: crate::object::PermissionFlags::EVERYONE,
            behavior: None,
        });
        area
    }

    #[test]
    fn parse_create_command_strips_options_from_name() {
        let parsed =
            parse_create_command("create container purse capacity=3 max_weight=10").unwrap();
        assert_eq!(parsed.type_name, "container");
        assert_eq!(parsed.display_name, "purse");
        assert_eq!(parsed.options.capacity, Some(3));
        assert_eq!(parsed.options.max_weight, Some(10));
    }

    #[test]
    fn parse_create_command_quoted_name_with_options() {
        let parsed =
            parse_create_command(r#"create container "Leather Bag" capacity=8 max_weight=40"#)
                .unwrap();
        assert_eq!(parsed.display_name, "Leather Bag");
        assert_eq!(parsed.options.capacity, Some(8));
    }

    #[test]
    fn parse_create_command_multi_word_name_without_options() {
        let parsed = parse_create_command("create sword Rusty Sword").unwrap();
        assert_eq!(parsed.type_name, "sword");
        assert_eq!(parsed.display_name, "Rusty Sword");
        assert_eq!(parsed.options, CreateOptions::default());
    }

    #[test]
    fn id_base_from_display_name_truncates_long_names() {
        let base = id_base_from_display_name("Extraordinarily Long Container Name");
        assert!(base.len() <= crate::object::ID_BASE_MAX_LEN);
    }

    #[test]
    fn parse_create_args_multi_word_name() {
        let input = "create sword Rusty Sword";
        let parts: Vec<&str> = input.split_whitespace().collect();
        let (ty, name) = parse_create_args(&parts, input).unwrap();
        assert_eq!(ty, "sword");
        assert_eq!(name, "Rusty Sword");
    }

    #[test]
    fn parse_create_args_quoted_name() {
        let input = r#"create sword "Rusty Sword""#;
        let parts: Vec<&str> = input.split_whitespace().collect();
        let (ty, name) = parse_create_args(&parts, input).unwrap();
        assert_eq!(ty, "sword");
        assert_eq!(name, "Rusty Sword");
    }

    #[test]
    fn slugify_produces_lowercase_hyphenated_id_base() {
        assert_eq!(slugify_display_name("Rusty Sword"), "rusty-sword");
        assert_eq!(
            slugify_display_name("  Big   Red  Boots  "),
            "big-red-boots"
        );
    }

    #[tokio::test]
    async fn create_multi_word_name_generates_lowercase_id() {
        let factory = test_factory().await;
        let anatomy = test_anatomy();
        let owner = ObjectId::new("player:hero-001");
        let area_id = ObjectId::new("area:the-void-001");

        let sword = create_at_location(
            &factory,
            "sword",
            "Rusty Sword",
            owner,
            Some(&area_id),
            &anatomy,
        )
        .await
        .unwrap();

        assert_eq!(sword.name, "Rusty Sword");
        assert_eq!(sword.id.as_str(), "sword:rusty-sword-001");
        assert_eq!(sword.location.as_ref(), Some(&area_id));
    }

    #[tokio::test]
    async fn create_item_placed_at_current_location() {
        let factory = test_factory().await;
        let anatomy = test_anatomy();
        let owner = ObjectId::new("player:hero-001");
        let area_id = ObjectId::new("area:the-void-001");
        let area = make_area(
            "area:the-void-001",
            "The Void",
            "A featureless void.",
            owner.clone(),
        );

        let boots = create_at_location(
            &factory,
            "item",
            "boots",
            owner.clone(),
            Some(&area_id),
            &anatomy,
        )
        .await
        .unwrap();

        assert_eq!(boots.location.as_ref(), Some(&area_id));
        assert_eq!(boots.object_type(), "item");

        let loaded = factory.load_object(&boots.id).await.unwrap().unwrap();
        assert_eq!(loaded.location.as_ref(), Some(&area_id));

        let mut objects = HashMap::new();
        objects.insert(area_id.clone(), area);
        objects.insert(boots.id.clone(), boots);

        let ctx =
            DisplayContext::new(owner.clone(), DisplayMode::Player).with_objects(objects.clone());
        let output = objects.get(&area_id).unwrap().describe(&ctx);
        assert!(output.contains("You see a boots here."));
    }

    #[tokio::test]
    async fn take_item_from_area_into_hand() {
        let factory = test_factory().await;
        let anatomy = test_anatomy();
        let owner = ObjectId::new("player:hero-001");
        let area_id = ObjectId::new("area:the-void-001");

        let mut player = factory
            .create_player("hero", owner.clone(), &anatomy)
            .await
            .unwrap();
        player.location = Some(area_id.clone());

        let mut boots = create_at_location(
            &factory,
            "item",
            "boots",
            owner.clone(),
            Some(&area_id),
            &anatomy,
        )
        .await
        .unwrap();
        boots.name = "Boots".to_string();

        let area = make_area(
            "area:the-void-001",
            "The Void",
            "A featureless void.",
            owner.clone(),
        );

        let mut objects = HashMap::new();
        objects.insert(player.id.clone(), player.clone());
        objects.insert(area_id.clone(), area);
        objects.insert(boots.id.clone(), boots);

        let msg =
            take_from_location(&owner, Some(&area_id), "boots", &mut objects, &anatomy).unwrap();
        assert_eq!(msg, "You pick up a Boots.");

        let player = objects.get(&owner).unwrap();
        assert!(
            player.body_slot_item("left_hand").is_some()
                || player.body_slot_item("right_hand").is_some()
        );

        let carried = describe_carried(player, &objects, &anatomy);
        assert!(carried.contains("Boots"));
    }

    #[tokio::test]
    async fn take_fails_when_item_not_visible() {
        let anatomy = test_anatomy();
        let owner = ObjectId::new("player:hero-001");
        let area_id = ObjectId::new("area:the-void-001");
        let mut objects = HashMap::new();

        let err = take_from_location(&owner, Some(&area_id), "boots", &mut objects, &anatomy)
            .unwrap_err();
        assert_eq!(err, InventoryError::NotFound("boots".to_string()));
    }

    #[tokio::test]
    async fn create_and_take_persist_through_hydrate() {
        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let factory = ObjectFactory::new(persistence.clone());
        let anatomy = test_anatomy();
        let owner = ObjectId::new("player:hero-001");
        let area_id = ObjectId::new("area:the-void-001");

        let mut player = factory
            .create_player("hero", owner.clone(), &anatomy)
            .await
            .unwrap();
        player.location = Some(area_id.clone());
        factory.persistence().save_object(&player).await.unwrap();

        let boots = create_at_location(
            &factory,
            "item",
            "boots",
            owner.clone(),
            Some(&area_id),
            &anatomy,
        )
        .await
        .unwrap();

        let mut objects = hydrate_world(&persistence).await.unwrap();
        take_from_location(&owner, Some(&area_id), "boots", &mut objects, &anatomy).unwrap();
        crate::world::persist_all(&persistence, &mut objects)
            .await
            .unwrap();

        let restored = hydrate_world(&persistence).await.unwrap();
        let player = restored.get(&owner).unwrap();
        assert!(
            player.body_slot_item("left_hand").is_some()
                || player.body_slot_item("right_hand").is_some()
        );
        let held_id = player
            .body_slot_item("right_hand")
            .or_else(|| player.body_slot_item("left_hand"))
            .unwrap();
        let held = restored.get(&held_id).unwrap();
        assert_eq!(held.name, boots.name);
        assert_eq!(held.location.as_ref(), Some(&owner));
    }

    #[tokio::test]
    async fn soft_delete_hides_object_until_undelete() {
        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let factory = ObjectFactory::new(persistence.clone());
        let anatomy = test_anatomy();
        let owner = ObjectId::new("player:hero-001");
        let area_id = ObjectId::new("area:the-void-001");

        let boots = create_at_location(
            &factory,
            "item",
            "boots",
            owner.clone(),
            Some(&area_id),
            &anatomy,
        )
        .await
        .unwrap();

        let mut cache = hydrate_world(&persistence).await.unwrap();
        soft_delete_object(&persistence, &boots.id, &mut cache)
            .await
            .unwrap();

        let visible = hydrate_world(&persistence).await.unwrap();
        assert!(!visible.contains_key(&boots.id));

        let loaded = persistence.load_object(&boots.id).await.unwrap().unwrap();
        assert!(loaded.is_deleted);

        undelete_object(&persistence, &boots.id, &mut cache)
            .await
            .unwrap();
        let restored = hydrate_world(&persistence).await.unwrap();
        assert!(restored.contains_key(&boots.id));
        assert!(restored.get(&boots.id).unwrap().is_active());
    }

    #[tokio::test]
    async fn immersive_create_take_look_self_flow() {
        let factory = test_factory().await;
        let anatomy = test_anatomy();
        let owner = ObjectId::new("player:hero-001");
        let area_id = ObjectId::new("area:the-void-001");
        let area = make_area(
            "area:the-void-001",
            "The Void",
            "A featureless void.",
            owner.clone(),
        );

        let mut player = factory
            .create_player("hero", owner.clone(), &anatomy)
            .await
            .unwrap();
        player.location = Some(area_id.clone());

        let sword = create_at_location(
            &factory,
            "sword",
            "Rusty Sword",
            owner.clone(),
            Some(&area_id),
            &anatomy,
        )
        .await
        .unwrap();

        let create_msg = narrate_create(&sword, Some(&area));
        assert!(create_msg.contains("Rusty Sword"));
        assert!(create_msg.contains("The Void"));
        assert!(!create_msg.contains(':'));

        let mut objects = HashMap::new();
        objects.insert(player.id.clone(), player.clone());
        objects.insert(area_id.clone(), area);
        objects.insert(sword.id.clone(), sword.clone());

        let take_msg = take_from_location(
            &owner,
            Some(&area_id),
            "rusty sword",
            &mut objects,
            &anatomy,
        )
        .unwrap();
        assert!(take_msg.contains("Rusty Sword"));
        assert!(!take_msg.contains(':'));

        let player = objects.get(&owner).unwrap();
        let ctx = DisplayContext::new(owner.clone(), DisplayMode::Player)
            .with_objects(objects.clone())
            .with_anatomy(anatomy.clone());
        let look_self = player.describe(&ctx);
        assert!(look_self.contains("Rusty Sword"));
        assert!(!look_self.contains("sword:rusty-sword"));
        assert!(!look_self.contains("player:hero"));

        let inventory = describe_carried(player, &objects, &anatomy);
        assert!(inventory.contains("Rusty Sword"));
        assert!(!inventory.contains(':'));
    }

    #[test]
    fn parse_create_command_parses_weight_and_volume() {
        let parsed =
            parse_create_command("create container backpack max_weight=100 weight=10 capacity=20")
                .unwrap();
        assert_eq!(parsed.type_name, "container");
        assert_eq!(parsed.display_name, "backpack");
        assert_eq!(parsed.options.capacity, Some(20));
        assert_eq!(parsed.options.max_weight, Some(100));
        assert_eq!(parsed.options.weight, Some(10.0));
    }

    #[test]
    fn parse_create_number_parses_int_and_decimal() {
        assert_eq!(parse_create_number("10"), Some(10.0));
        assert_eq!(parse_create_number("0.1"), Some(0.1));
        assert_eq!(parse_create_number("abc"), None);
        assert_eq!(parse_create_number(""), None);
    }

    #[test]
    fn parse_create_command_parses_decimal_weight() {
        let parsed =
            parse_create_command("create stackable coins weight=0.1 stack_count=21").unwrap();
        assert_eq!(parsed.type_name, "stackable");
        assert_eq!(parsed.options.weight, Some(0.1));
        assert_eq!(parsed.options.stack_count, Some(21));
    }

    #[tokio::test]
    async fn create_stackable_decimal_weight_and_examine_status() {
        let factory = test_factory().await;
        let anatomy = test_anatomy();
        let owner = ObjectId::new("player:hero-001");

        let parsed =
            parse_create_command("create stackable coins weight=0.1 stack_count=21").unwrap();
        let coins = create_at_location_with_options(
            &factory,
            &parsed.type_name,
            &parsed.display_name,
            owner.clone(),
            None,
            &anatomy,
            parsed.options,
        )
        .await
        .unwrap();

        assert!((coins.unit_weight() - 0.1).abs() < f64::EPSILON);
        assert!((coins.weight() - 2.1).abs() < f64::EPSILON);

        let mut objects = HashMap::new();
        objects.insert(coins.id.clone(), coins.clone());
        let builder_ctx = DisplayContext::new(owner, DisplayMode::Builder).with_objects(objects);
        let examine_out = coins.describe_detailed(&builder_ctx);
        assert!(examine_out.contains("weight: 0.1"));
        assert!(examine_out.contains("weight: 2.1"));
    }

    #[tokio::test]
    async fn create_container_applies_weight_to_examine() {
        let factory = test_factory().await;
        let anatomy = test_anatomy();
        let owner = ObjectId::new("player:hero-001");

        let parsed =
            parse_create_command("create container backpack max_weight=100 weight=10 capacity=20")
                .unwrap();
        let backpack = create_at_location_with_options(
            &factory,
            &parsed.type_name,
            &parsed.display_name,
            owner.clone(),
            None,
            &anatomy,
            parsed.options,
        )
        .await
        .unwrap();

        assert!((backpack.weight() - 10.0).abs() < f64::EPSILON);
        assert_eq!(backpack.container_capacity(), 20);
        assert_eq!(backpack.container_max_weight(), Some(100));

        let mut objects = HashMap::new();
        objects.insert(backpack.id.clone(), backpack.clone());
        let builder_ctx = DisplayContext::new(owner, DisplayMode::Builder).with_objects(objects);
        let examine_out = backpack.describe_detailed(&builder_ctx);
        assert!(examine_out.contains("properties:"));
        assert!(examine_out.contains("weight: 10"));
        assert!(examine_out.contains("state:"));
        assert!(examine_out.contains("status:"));
        assert!(examine_out.contains("contents_weight: 0/100"));
        assert!(examine_out.contains("type: container"));
    }

    #[tokio::test]
    async fn create_container_with_params_uses_clean_name_and_id() {
        let factory = test_factory().await;
        let anatomy = test_anatomy();
        let owner = ObjectId::new("player:hero-001");
        let area_id = ObjectId::new("area:the-void-001");

        let parsed =
            parse_create_command("create container purse capacity=3 max_weight=10").unwrap();
        let purse = create_at_location_with_options(
            &factory,
            &parsed.type_name,
            &parsed.display_name,
            owner,
            Some(&area_id),
            &anatomy,
            parsed.options,
        )
        .await
        .unwrap();

        assert_eq!(purse.name, "purse");
        assert!(purse.id.as_str().starts_with("item:purse-"));
        assert!(!purse.id.as_str().contains("capacity"));
        assert_eq!(purse.container_capacity(), 3);
        assert_eq!(purse.container_max_weight(), Some(10));
    }

    #[tokio::test]
    async fn wizard_create_container_with_capacity() {
        let factory = test_factory().await;
        let anatomy = test_anatomy();
        let owner = ObjectId::new("player:hero-001");
        let area_id = ObjectId::new("area:the-void-001");

        let opts = CreateOptions {
            capacity: Some(3),
            max_weight: Some(25),
            ..Default::default()
        };

        let bag = create_at_location_with_options(
            &factory,
            "container",
            "Leather Bag",
            owner,
            Some(&area_id),
            &anatomy,
            opts,
        )
        .await
        .unwrap();

        assert!(bag.is_container());
        assert_eq!(bag.container_capacity(), 3);
        assert_eq!(bag.container_max_weight(), Some(25));
    }

    #[test]
    fn examine_player_vs_meta_builder_output() {
        use crate::display::{Describable, DisplayContext, DisplayMode};

        let owner = ObjectId::new("player:hero-001");
        let mut coins = Object {
            id: ObjectId::new("item:coins-001"),
            name: "coins".to_string(),
            aliases: Vec::new(),
            location: Some(ObjectId::new("room:test-001")),
            prototype: None,
            owner: owner.clone(),
            permissions: crate::object::PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            revision: 0,
            updated_at: None,
            is_deleted: false,
            deleted_at: None,
        };
        coins.apply_stackable_role(&crate::object::StackableSpec {
            count: 10,
            max_stack: 99,
        });
        coins.add_property(crate::object::Property {
            name: "description".to_string(),
            value: crate::object::Value::String("Shiny.".to_string()),
            permissions: crate::object::PermissionFlags::EVERYONE,
            behavior: None,
        });
        coins.add_verb(crate::object::Verb {
            name: "flip".to_string(),
            code: "say('flip')".to_string(),
            permissions: crate::object::PermissionFlags::EVERYONE,
        });

        let mut objects = HashMap::new();
        objects.insert(coins.id.clone(), coins.clone());

        let player_ctx =
            DisplayContext::new(owner.clone(), DisplayMode::Player).with_objects(objects.clone());
        let player_out = coins.describe(&player_ctx);
        assert!(player_out.contains("Shiny."));
        assert!(player_out.contains("weighs 10 in total"));
        assert!(!player_out.contains("id:"));
        assert!(!player_out.contains("properties:"));

        let builder_ctx = DisplayContext::new(owner, DisplayMode::Builder).with_objects(objects);
        let builder_out = coins.describe_detailed(&builder_ctx);
        assert!(builder_out.contains("id: coins-001"));
        assert!(builder_out.contains("properties:"));
        assert!(builder_out.contains("state:"));
        assert!(builder_out.contains("flip"));
    }

    #[test]
    fn meta_command_parser_strips_at_prefix() {
        let line = parse_command_line("@examine purse");
        assert!(line.is_meta);
        assert_eq!(line.verb, "examine");
        assert_eq!(line.args, vec!["purse".to_string()]);
    }

    #[tokio::test]
    async fn wizard_create_stackable_item() {
        let factory = test_factory().await;
        let anatomy = test_anatomy();
        let owner = ObjectId::new("player:hero-001");

        let opts = CreateOptions {
            stack_count: Some(42),
            ..Default::default()
        };

        let coins = create_at_location_with_options(
            &factory,
            "stackable",
            "Gold Coin",
            owner,
            None,
            &anatomy,
            opts,
        )
        .await
        .unwrap();

        assert!(coins.is_stackable());
        assert_eq!(coins.stack_count(), 42);
    }

    #[tokio::test]
    async fn wizard_set_unset_property_and_verb() {
        let factory = test_factory().await;
        let owner = ObjectId::new("player:hero-001");

        let mut backpack = factory
            .create_container_with_spec(
                "backpack",
                owner.clone(),
                ContainerSpec {
                    capacity: 20,
                    max_weight: Some(100),
                    max_volume: None,
                    wearable: true,
                    wear_slot: Some("torso".to_string()),
                    ..crate::object::ContainerSpec::default()
                },
                None,
            )
            .await
            .unwrap();

        let mut objects = HashMap::new();
        objects.insert(backpack.id.clone(), backpack.clone());
        objects.insert(owner.clone(), bare_player(&owner));

        apply_set(&mut backpack, "weight", "10", &owner, &objects).unwrap();
        apply_set(
            &mut backpack,
            "verb.polish",
            "say('You polish it.')",
            &owner,
            &objects,
        )
        .unwrap();

        assert!((backpack.weight() - 10.0).abs() < f64::EPSILON);
        assert!(backpack.verbs.contains_key("polish"));

        apply_unset(&mut backpack, "verb.polish").unwrap();
        apply_unset(&mut backpack, "weight").unwrap();

        assert!(!backpack.verbs.contains_key("polish"));
        assert!(backpack.get_numeric_property("weight").is_none());
    }

    fn bare_player(id: &ObjectId) -> Object {
        Object {
            id: id.clone(),
            name: "Hero".to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: id.clone(),
            permissions: crate::object::PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            revision: 0,
            updated_at: None,
            is_deleted: false,
            deleted_at: None,
        }
    }

    #[tokio::test]
    async fn take_fails_when_hands_full() {
        let factory = test_factory().await;
        let anatomy = test_anatomy();
        let owner = ObjectId::new("player:hero-001");
        let area_id = ObjectId::new("area:the-void-001");

        let mut player = factory
            .create_player("hero", owner.clone(), &anatomy)
            .await
            .unwrap();
        player.location = Some(area_id.clone());

        let mut sword = factory.create_item("sword", owner.clone()).await.unwrap();
        sword.name = "Sword".to_string();
        sword.set_property_string("hand_slot", "both");
        sword.location = Some(area_id.clone());

        let mut axe = factory.create_item("axe", owner.clone()).await.unwrap();
        axe.name = "Axe".to_string();
        axe.location = Some(area_id.clone());

        let mut objects = HashMap::new();
        objects.insert(player.id.clone(), player);
        objects.insert(sword.id.clone(), sword);
        objects.insert(axe.id.clone(), axe);

        take_from_location(&owner, Some(&area_id), "sword", &mut objects, &anatomy).unwrap();
        let err =
            take_from_location(&owner, Some(&area_id), "axe", &mut objects, &anatomy).unwrap_err();
        assert_eq!(err, InventoryError::HandsFull);
    }
}
