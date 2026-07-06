//! Centralized object lookup with possession priority and disambiguation.
//!
//! Search order (for [`ResolveScope::General`] and [`ResolveScope::PossessionOrRoom`]):
//! 1. Immediate possession (body slots)
//! 2. Nested containers carried/worn by the player (BFS, no deep recursion)
//! 3. Ground in the current room (player-owned first)
//! 4. Global fallback (any active object)

use std::collections::{HashMap, VecDeque};

use crate::mudl::slot_display_name;
use crate::object::{Object, ObjectId};

use super::stackable::stack_quantity_phrase;

/// Result of resolving a player-typed object name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetResolution {
    Found(ObjectId),
    Ambiguous(String),
    NotFound,
}

/// Where to search when resolving a target name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveScope {
    /// Possession tiers, then room (player-owned, then any), then global.
    General,
    /// Body slots and nested carried containers only.
    PossessionOnly,
    /// Ground in the current room only (excludes carried items).
    RoomOnly,
    /// Possession tiers first, then room ground.
    PossessionOrRoom,
}

/// A name match with an optional location hint for disambiguation prompts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedMatch {
    pub id: ObjectId,
    pub location_hint: Option<String>,
}

/// Suffix after the type prefix (e.g. `item:coins-042` → `coins-042`).
pub fn short_id(id: &ObjectId) -> String {
    id.as_str()
        .split_once(':')
        .map(|(_, rest)| rest.to_string())
        .unwrap_or_else(|| id.as_str().to_string())
}

/// Whether typed input is intended as an object id reference (not a plain name).
pub fn looks_like_id_reference(name: &str) -> bool {
    if name.contains(':') {
        return true;
    }
    name.contains('-') && name.chars().any(|c| c.is_ascii_digit())
}

/// Candidate object ids for a player-typed reference (`gold-bar-001` → `item:gold-bar-001`).
pub fn id_lookup_candidates(name: &str) -> Vec<ObjectId> {
    let mut out = vec![ObjectId::new(name)];
    if !name.contains(':') {
        for prefix in ["item", "player", "room", "area", "creature", "prototype"] {
            out.push(ObjectId::new(format!("{prefix}:{name}")));
        }
    }
    out
}

fn object_visible_in_scope(
    id: &ObjectId,
    player_id: &ObjectId,
    room_id: Option<&ObjectId>,
    objects: &HashMap<ObjectId, Object>,
    scope: ResolveScope,
) -> bool {
    id_in_scope(id, player_id, room_id, objects, scope) || scope == ResolveScope::General
}

/// Location/count hint for disambiguation lines (`6 gold bars`, `in your right hand`).
fn disambiguation_hint(
    obj: &Object,
    obj_id: &ObjectId,
    player_id: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
    room_id: Option<&ObjectId>,
) -> String {
    if let Some(player) = objects.get(player_id) {
        if let Some(carry) = direct_carry_hint(player, obj_id) {
            if obj.is_stackable() {
                return format!("{}, {carry}", stack_quantity_phrase(obj));
            }
            return carry;
        }
    }
    if obj.is_stackable() {
        return stack_quantity_phrase(obj);
    }
    if room_id.is_some_and(|room| obj.location.as_ref() == Some(room)) {
        return "here".to_string();
    }
    short_id(obj_id)
}

fn resolve_by_object_id(
    name: &str,
    player_id: &ObjectId,
    room_id: Option<&ObjectId>,
    objects: &HashMap<ObjectId, Object>,
    scope: ResolveScope,
) -> Option<TargetResolution> {
    for candidate in id_lookup_candidates(name) {
        if !objects.contains_key(&candidate) {
            continue;
        }
        if object_visible_in_scope(&candidate, player_id, room_id, objects, scope) {
            return Some(TargetResolution::Found(candidate));
        }
        if looks_like_id_reference(name) {
            return Some(TargetResolution::NotFound);
        }
    }

    if !looks_like_id_reference(name) {
        return None;
    }

    let needle = name.to_ascii_lowercase();
    let mut matches: Vec<ResolvedMatch> = objects
        .iter()
        .filter(|(id, obj)| {
            obj.is_active()
                && object_visible_in_scope(id, player_id, room_id, objects, scope)
                && short_id(id).eq_ignore_ascii_case(&needle)
        })
        .map(|(id, obj)| ResolvedMatch {
            id: id.clone(),
            location_hint: Some(disambiguation_hint(
                obj, id, player_id, objects, room_id,
            )),
        })
        .collect();

    matches.sort_by(|a, b| short_id(&a.id).cmp(&short_id(&b.id)));

    Some(match matches.len() {
        0 => TargetResolution::NotFound,
        1 => TargetResolution::Found(matches[0].id.clone()),
        _ => TargetResolution::Ambiguous(format_disambiguation(name, &matches)),
    })
}

/// Whether a typed name matches an object's display name or aliases.
pub fn name_matches(needle: &str, obj: &Object) -> bool {
    let name_lower = obj.name.to_lowercase();
    name_lower == needle
        || name_lower.contains(needle)
        || name_lower
            .split_whitespace()
            .any(|word| word == needle || word.starts_with(needle))
        || obj.aliases.iter().any(|a| {
            let alias = a.to_lowercase();
            alias == needle || alias.contains(needle)
        })
}

/// Whether `item_id` is on the player's body or inside a carried/worn container (BFS).
pub fn is_in_player_possession(
    player_id: &ObjectId,
    item_id: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
) -> bool {
    if item_id == player_id {
        return true;
    }

    let Some(player) = objects.get(player_id) else {
        return false;
    };

    if player.body_slots().values().any(|id| id == item_id) {
        return true;
    }

    let mut queue: VecDeque<ObjectId> = player.carried_body_items().into_iter().collect();
    let mut visited = HashMap::new();

    while let Some(container_id) = queue.pop_front() {
        if visited.contains_key(&container_id) {
            continue;
        }
        visited.insert(container_id.clone(), ());

        let Some(container) = objects.get(&container_id) else {
            continue;
        };
        if !container.is_container() {
            continue;
        }

        for content_id in container.container_contents() {
            if &content_id == item_id {
                return true;
            }
            if objects
                .get(&content_id)
                .is_some_and(|obj| obj.is_container())
            {
                queue.push_back(content_id);
            }
        }
    }

    false
}

fn is_on_ground_in_room(
    obj: &Object,
    obj_id: &ObjectId,
    room_id: &ObjectId,
    player_id: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
) -> bool {
    obj.location.as_ref() == Some(room_id) && !is_in_player_possession(player_id, obj_id, objects)
}

fn direct_carry_hint(player: &Object, item_id: &ObjectId) -> Option<String> {
    for (slot, id) in player.body_slots() {
        if id == *item_id {
            return Some(format!("in your {}", slot_display_name(&slot)));
        }
    }
    None
}

fn collect_possession_matches(
    needle: &str,
    player_id: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
) -> (Vec<ResolvedMatch>, Vec<ResolvedMatch>) {
    let mut immediate = Vec::new();
    let mut nested = Vec::new();

    let Some(player) = objects.get(player_id) else {
        return (immediate, nested);
    };

    for item_id in player.carried_body_items() {
        let Some(obj) = objects.get(&item_id) else {
            continue;
        };
        if !obj.is_active() || !name_matches(needle, obj) {
            continue;
        }
        immediate.push(ResolvedMatch {
            id: item_id.clone(),
            location_hint: Some(disambiguation_hint(
                obj, &item_id, player_id, objects, None,
            )),
        });
    }

    let mut queue: VecDeque<(ObjectId, String)> = VecDeque::new();
    let mut visited = HashMap::new();

    for container_id in player.carried_body_items() {
        let Some(container) = objects.get(&container_id) else {
            continue;
        };
        if !container.is_container() {
            continue;
        }
        let hint = container.name.to_lowercase();
        for content_id in container.container_contents() {
            queue.push_back((content_id, hint.clone()));
        }
        visited.insert(container_id, ());
    }

    while let Some((item_id, container_hint)) = queue.pop_front() {
        let Some(obj) = objects.get(&item_id) else {
            continue;
        };
        if !obj.is_active() {
            continue;
        }

        if name_matches(needle, obj) {
            nested.push(ResolvedMatch {
                id: item_id.clone(),
                location_hint: Some(if obj.is_stackable() {
                    format!("{}, in {container_hint}", stack_quantity_phrase(obj))
                } else {
                    format!("in {container_hint}")
                }),
            });
        }

        if obj.is_container() && !visited.contains_key(&item_id) {
            visited.insert(item_id.clone(), ());
            let hint = obj.name.to_lowercase();
            for inner_id in obj.container_contents() {
                queue.push_back((inner_id, hint.clone()));
            }
        }
    }

    (immediate, nested)
}

fn collect_room_matches(
    needle: &str,
    room_id: &ObjectId,
    player_id: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
) -> (Vec<ResolvedMatch>, Vec<ResolvedMatch>) {
    let mut player_owned = Vec::new();
    let mut other = Vec::new();

    for (obj_id, obj) in objects {
        if !obj.is_active() || !name_matches(needle, obj) {
            continue;
        }
        if !is_on_ground_in_room(obj, obj_id, room_id, player_id, objects) {
            continue;
        }
        let m = ResolvedMatch {
            id: obj_id.clone(),
            location_hint: Some(disambiguation_hint(
                obj, obj_id, player_id, objects, Some(room_id),
            )),
        };
        if obj.owner == *player_id {
            player_owned.push(m);
        } else {
            other.push(m);
        }
    }

    (player_owned, other)
}

fn collect_global_matches(needle: &str, objects: &HashMap<ObjectId, Object>) -> Vec<ResolvedMatch> {
    let mut matches = Vec::new();
    for (obj_id, obj) in objects {
        if obj.is_active() && name_matches(needle, obj) {
            matches.push(ResolvedMatch {
                id: obj_id.clone(),
                location_hint: None,
            });
        }
    }
    matches
}

fn resolve_by_name(
    needle: &str,
    player_id: &ObjectId,
    room_id: Option<&ObjectId>,
    objects: &HashMap<ObjectId, Object>,
    scope: ResolveScope,
) -> TargetResolution {
    let finish = |matches: Vec<ResolvedMatch>, name: &str| -> TargetResolution {
        match matches.len() {
            0 => TargetResolution::NotFound,
            1 => TargetResolution::Found(matches[0].id.clone()),
            _ => TargetResolution::Ambiguous(format_disambiguation(name, &matches)),
        }
    };

    let search_possession = matches!(
        scope,
        ResolveScope::General | ResolveScope::PossessionOnly | ResolveScope::PossessionOrRoom
    );
    let search_room = matches!(
        scope,
        ResolveScope::General | ResolveScope::RoomOnly | ResolveScope::PossessionOrRoom
    );
    let search_global = scope == ResolveScope::General;

    if search_possession {
        let (immediate, nested) = collect_possession_matches(needle, player_id, objects);
        let possession = if !immediate.is_empty() {
            immediate
        } else {
            nested
        };
        if !possession.is_empty() {
            return finish(possession, needle);
        }
        if scope == ResolveScope::PossessionOnly {
            return TargetResolution::NotFound;
        }
    }

    if search_room {
        if let Some(room_id) = room_id {
            let (owned, other) = collect_room_matches(needle, room_id, player_id, objects);
            let room_matches = if !owned.is_empty() { owned } else { other };
            if !room_matches.is_empty() {
                return finish(room_matches, needle);
            }
        }
        if scope == ResolveScope::RoomOnly || scope == ResolveScope::PossessionOrRoom {
            return TargetResolution::NotFound;
        }
    }

    if search_global {
        return finish(collect_global_matches(needle, objects), needle);
    }

    TargetResolution::NotFound
}

/// Build a disambiguation prompt listing short IDs and container locations.
pub fn format_disambiguation(name: &str, matches: &[ResolvedMatch]) -> String {
    let mut lines = vec![format!("Which {name} do you mean?")];
    for m in matches {
        let hint = m
            .location_hint
            .as_ref()
            .map(|h| format!(" ({h})"))
            .unwrap_or_default();
        lines.push(format!("  {}{}", short_id(&m.id), hint));
    }
    lines.join("\n")
}

fn id_in_scope(
    id: &ObjectId,
    player_id: &ObjectId,
    room_id: Option<&ObjectId>,
    objects: &HashMap<ObjectId, Object>,
    scope: ResolveScope,
) -> bool {
    let Some(obj) = objects.get(id) else {
        return false;
    };
    if !obj.is_active() {
        return false;
    }

    match scope {
        ResolveScope::PossessionOnly => is_in_player_possession(player_id, id, objects),
        ResolveScope::RoomOnly => room_id.is_some_and(|room| {
            is_on_ground_in_room(obj, id, room, player_id, objects)
        }),
        ResolveScope::PossessionOrRoom => {
            is_in_player_possession(player_id, id, objects)
                || room_id.is_some_and(|room| obj.location.as_ref() == Some(room))
        }
        ResolveScope::General => true,
    }
}

/// Resolve a player-typed target name to a single object, with disambiguation when needed.
pub fn resolve_object(
    name: &str,
    player_id: &ObjectId,
    room_id: Option<&ObjectId>,
    objects: &HashMap<ObjectId, Object>,
    scope: ResolveScope,
) -> TargetResolution {
    let needle = name.to_lowercase();

    if needle == "self" || needle == "me" {
        return TargetResolution::Found(player_id.clone());
    }

    if needle == "here" {
        return room_id
            .cloned()
            .map(TargetResolution::Found)
            .unwrap_or(TargetResolution::NotFound);
    }

    if let Some(result) =
        resolve_by_object_id(name, player_id, room_id, objects, scope)
    {
        return result;
    }

    resolve_by_name(&needle, player_id, room_id, objects, scope)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{ContainerSpec, PermissionFlags, StackableSpec};

    fn bare(id: &str, name: &str, owner: &ObjectId) -> Object {
        Object {
            id: ObjectId::new(id),
            name: name.to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: owner.clone(),
            permissions: PermissionFlags::OWNER,
            properties: Default::default(),
            verbs: Default::default(),
            event_handlers: Default::default(),
            is_deleted: false,
            deleted_at: None,
        }
    }

    fn setup_player_with_nested_coins() -> (ObjectId, ObjectId, HashMap<ObjectId, Object>) {
        let player_id = ObjectId::new("player:hero-001");
        let room_id = ObjectId::new("room:test-001");

        let mut player = bare("player:hero-001", "Hero", &player_id);
        player.location = Some(room_id.clone());

        let mut purse = bare("item:purse-001", "purse", &player_id);
        purse.apply_container_role(&ContainerSpec {
            capacity: 3,
            max_weight: None,
            max_volume: None,
            wearable: true,
            wear_slot: Some("torso".to_string()),
        });

        let mut backpack = bare("item:backpack-001", "backpack", &player_id);
        backpack.apply_container_role(&ContainerSpec {
            capacity: 5,
            max_weight: None,
            max_volume: None,
            wearable: true,
            wear_slot: Some("torso".to_string()),
        });

        let mut coins_purse = bare("item:coins-001", "coins", &player_id);
        coins_purse.apply_stackable_role(&StackableSpec {
            count: 10,
            max_stack: 99,
        });
        coins_purse.location = Some(purse.id.clone());

        let mut coins_backpack = bare("item:coins-002", "coins", &player_id);
        coins_backpack.apply_stackable_role(&StackableSpec {
            count: 5,
            max_stack: 99,
        });
        coins_backpack.location = Some(backpack.id.clone());

        purse.set_property_list("contents", vec![coins_purse.id.clone()]);
        backpack.set_property_list("contents", vec![coins_backpack.id.clone()]);

        player.set_body_slot("torso", Some(purse.id.clone()));
        player.set_body_slot("back", Some(backpack.id.clone()));

        let mut objects = HashMap::new();
        objects.insert(room_id.clone(), bare("room:test-001", "Test Room", &player_id));
        objects.insert(player_id.clone(), player);
        objects.insert(purse.id.clone(), purse);
        objects.insert(backpack.id.clone(), backpack);
        objects.insert(coins_purse.id.clone(), coins_purse);
        objects.insert(coins_backpack.id.clone(), coins_backpack);

        (player_id, room_id, objects)
    }

    #[test]
    fn short_id_strips_type_prefix() {
        assert_eq!(short_id(&ObjectId::new("item:coins-042")), "coins-042");
    }

    #[test]
    fn looks_like_id_reference_detects_short_ids() {
        assert!(looks_like_id_reference("gold-bar-001"));
        assert!(looks_like_id_reference("gold-bar-001-s001"));
        assert!(looks_like_id_reference("item:coins-042"));
        assert!(!looks_like_id_reference("gold bar"));
        assert!(!looks_like_id_reference("gold bars"));
    }

    #[test]
    fn resolve_by_short_id_on_ground() {
        let player_id = ObjectId::new("player:hero-001");
        let room_id = ObjectId::new("room:test-001");

        let mut stack = bare("item:gold-bar-001", "gold bar", &player_id);
        stack.location = Some(room_id.clone());
        stack.apply_stackable_role(&StackableSpec {
            count: 10,
            max_stack: 99,
        });

        let mut split = stack.clone();
        split.id = ObjectId::new("item:gold-bar-001-s001");
        split.set_stack_count(1);

        let mut objects = HashMap::new();
        objects.insert(room_id.clone(), bare("room:test-001", "Test", &player_id));
        objects.insert(stack.id.clone(), stack);
        objects.insert(split.id.clone(), split.clone());

        let ambiguous = resolve_object(
            "gold bar",
            &player_id,
            Some(&room_id),
            &objects,
            ResolveScope::RoomOnly,
        );
        assert!(matches!(ambiguous, TargetResolution::Ambiguous(_)));

        let by_id = resolve_object(
            "gold-bar-001",
            &player_id,
            Some(&room_id),
            &objects,
            ResolveScope::RoomOnly,
        );
        assert_eq!(
            by_id,
            TargetResolution::Found(ObjectId::new("item:gold-bar-001"))
        );

        let by_split = resolve_object(
            "gold-bar-001-s001",
            &player_id,
            Some(&room_id),
            &objects,
            ResolveScope::RoomOnly,
        );
        assert_eq!(by_split, TargetResolution::Found(split.id));
    }

    #[test]
    fn disambiguation_lists_short_ids_with_stack_counts() {
        let player_id = ObjectId::new("player:hero-001");
        let room_id = ObjectId::new("room:test-001");

        let mut stack = bare("item:gold-bar-001", "gold bar", &player_id);
        stack.location = Some(room_id.clone());
        stack.apply_stackable_role(&StackableSpec {
            count: 10,
            max_stack: 99,
        });

        let mut split = bare("item:gold-bar-001-s001", "gold bar", &player_id);
        split.location = Some(room_id.clone());
        split.apply_stackable_role(&StackableSpec {
            count: 1,
            max_stack: 99,
        });

        let mut objects = HashMap::new();
        objects.insert(room_id.clone(), bare("room:test-001", "Test", &player_id));
        objects.insert(stack.id.clone(), stack);
        objects.insert(split.id.clone(), split);

        let result = resolve_object(
            "gold bar",
            &player_id,
            Some(&room_id),
            &objects,
            ResolveScope::RoomOnly,
        );
        match result {
            TargetResolution::Ambiguous(msg) => {
                assert!(msg.contains("gold-bar-001 (10 gold bars)"));
                assert!(msg.contains("gold-bar-001-s001 (gold bar)"));
                assert!(!msg.contains(", here"));
            }
            other => panic!("expected ambiguous, got {other:?}"),
        }
    }

    #[test]
    fn possession_prefers_immediate_over_nested() {
        let (player_id, room_id, mut objects) = setup_player_with_nested_coins();

        let mut coins_hand = bare("item:coins-003", "coins", &player_id);
        coins_hand.apply_stackable_role(&StackableSpec {
            count: 1,
            max_stack: 99,
        });
        coins_hand.location = Some(player_id.clone());
        let hand_id = coins_hand.id.clone();

        let player = objects.get_mut(&player_id).unwrap();
        player.set_body_slot("right_hand", Some(hand_id.clone()));
        objects.insert(hand_id.clone(), coins_hand);

        let result = resolve_object(
            "coins",
            &player_id,
            Some(&room_id),
            &objects,
            ResolveScope::PossessionOnly,
        );
        assert_eq!(result, TargetResolution::Found(hand_id));
    }

    #[test]
    fn nested_possession_disambiguates_with_container_hints() {
        let (player_id, room_id, objects) = setup_player_with_nested_coins();

        let result = resolve_object(
            "coins",
            &player_id,
            Some(&room_id),
            &objects,
            ResolveScope::PossessionOnly,
        );

        match result {
            TargetResolution::Ambiguous(msg) => {
                assert!(msg.contains("Which coins do you mean?"));
                assert!(msg.contains("coins-001 (10 coins, in purse)"));
                assert!(msg.contains("coins-002 (5 coins, in backpack)"));
            }
            other => panic!("expected ambiguous, got {other:?}"),
        }
    }

    #[test]
    fn room_search_prefers_player_owned_on_ground() {
        let player_id = ObjectId::new("player:hero-001");
        let other_owner = ObjectId::new("player:other-001");
        let room_id = ObjectId::new("room:test-001");

        let mut owned = bare("item:sword-001", "sword", &player_id);
        owned.location = Some(room_id.clone());

        let mut other = bare("item:sword-002", "sword", &other_owner);
        other.location = Some(room_id.clone());

        let mut objects = HashMap::new();
        objects.insert(room_id.clone(), bare("room:test-001", "Test", &player_id));
        objects.insert(owned.id.clone(), owned.clone());
        objects.insert(other.id.clone(), other);

        let result = resolve_object(
            "sword",
            &player_id,
            Some(&room_id),
            &objects,
            ResolveScope::RoomOnly,
        );
        assert_eq!(result, TargetResolution::Found(owned.id));
    }

    #[test]
    fn general_scope_falls_back_to_global() {
        let player_id = ObjectId::new("player:hero-001");
        let room_id = ObjectId::new("room:test-001");

        let mut distant = bare("item:orb-001", "orb", &player_id);
        distant.location = Some(ObjectId::new("room:far-001"));

        let mut objects = HashMap::new();
        objects.insert(room_id.clone(), bare("room:test-001", "Test", &player_id));
        objects.insert(distant.id.clone(), distant.clone());

        let result = resolve_object(
            "orb",
            &player_id,
            Some(&room_id),
            &objects,
            ResolveScope::General,
        );
        assert_eq!(result, TargetResolution::Found(distant.id));
    }

    #[test]
    fn bfs_finds_deeply_nested_items() {
        let player_id = ObjectId::new("player:hero-001");

        let mut bag = bare("item:bag-001", "bag", &player_id);
        bag.apply_container_role(&ContainerSpec {
            capacity: 3,
            max_weight: None,
            max_volume: None,
            wearable: false,
            wear_slot: None,
        });

        let mut pouch = bare("item:pouch-001", "pouch", &player_id);
        pouch.apply_container_role(&ContainerSpec {
            capacity: 2,
            max_weight: None,
            max_volume: None,
            wearable: false,
            wear_slot: None,
        });

        let mut gem = bare("item:gem-001", "gem", &player_id);
        gem.location = Some(pouch.id.clone());
        pouch.set_property_list("contents", vec![gem.id.clone()]);
        pouch.location = Some(bag.id.clone());
        bag.set_property_list("contents", vec![pouch.id.clone()]);

        let mut player = bare("player:hero-001", "Hero", &player_id);
        player.set_body_slot("right_hand", Some(bag.id.clone()));

        let mut objects = HashMap::new();
        objects.insert(player_id.clone(), player);
        objects.insert(bag.id.clone(), bag);
        objects.insert(pouch.id.clone(), pouch);
        let gem_id = gem.id.clone();
        objects.insert(gem_id.clone(), gem);

        assert!(is_in_player_possession(&player_id, &gem_id, &objects));
    }

    #[test]
    fn resolve_by_full_typed_id() {
        let player_id = ObjectId::new("player:hero-001");
        let room_id = ObjectId::new("room:test-001");

        let mut sword = bare("item:sword-001", "sword", &player_id);
        sword.location = Some(room_id.clone());

        let mut objects = HashMap::new();
        objects.insert(room_id.clone(), bare("room:test-001", "Test", &player_id));
        objects.insert(sword.id.clone(), sword);

        let result = resolve_object(
            "item:sword-001",
            &player_id,
            Some(&room_id),
            &objects,
            ResolveScope::RoomOnly,
        );
        assert_eq!(result, TargetResolution::Found(ObjectId::new("item:sword-001")));
    }

    #[test]
    fn resolve_by_short_id_not_found_when_absent() {
        let player_id = ObjectId::new("player:hero-001");
        let room_id = ObjectId::new("room:test-001");

        let objects = HashMap::from([(
            room_id.clone(),
            bare("room:test-001", "Test", &player_id),
        )]);

        let result = resolve_object(
            "missing-item-999",
            &player_id,
            Some(&room_id),
            &objects,
            ResolveScope::RoomOnly,
        );
        assert_eq!(result, TargetResolution::NotFound);
    }

    #[test]
    fn resolve_duplicate_names_requires_disambiguation_hint() {
        let player_id = ObjectId::new("player:hero-001");
        let room_id = ObjectId::new("room:test-001");

        let mut first = bare("item:coin-001", "coin", &player_id);
        first.location = Some(room_id.clone());
        let mut second = bare("item:coin-002", "coin", &player_id);
        second.location = Some(room_id.clone());

        let mut objects = HashMap::new();
        objects.insert(room_id.clone(), bare("room:test-001", "Test", &player_id));
        objects.insert(first.id.clone(), first);
        objects.insert(second.id.clone(), second);

        let result = resolve_object(
            "coin",
            &player_id,
            Some(&room_id),
            &objects,
            ResolveScope::RoomOnly,
        );
        match result {
            TargetResolution::Ambiguous(msg) => {
                assert!(msg.contains("coin-001"));
                assert!(msg.contains("coin-002"));
                assert!(msg.contains("Which coin do you mean?"));
            }
            other => panic!("expected ambiguous, got {other:?}"),
        }
    }
}