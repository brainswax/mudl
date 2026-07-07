//! Natural-language `examine self` — creature identity, gear, slot use, and weight.

use std::collections::HashMap;

use crate::display::equipment::{collect_gear_lists, occupied_body_slots};
use crate::display::grammar::phrase_with_leading_article;
use crate::mudl::{slot_display_name, AnatomyRegistry, BodyPlan};
use crate::object::{
    format_weight_amount, is_unlimited_weight, player_carried_weight, player_effective_max_weight,
    Object, ObjectId,
};

/// Placement label for equipped items (torso → "back", per common MUD convention).
pub fn equipment_placement_label(slot: &str) -> String {
    match slot {
        "torso" => "back".to_string(),
        other => slot_display_name(other),
    }
}

fn format_identity_sentence(creature: &str, holding: &[String], wearing: &[String]) -> String {
    let mut sentence = format!("You're a {creature}");
    match (holding.is_empty(), wearing.is_empty()) {
        (true, true) => sentence.push('.'),
        (false, true) => {
            sentence.push_str(&format!(
                " carrying {}.",
                phrase_with_leading_article(holding)
            ));
        }
        (true, false) => {
            sentence.push_str(&format!(
                " wearing {}.",
                phrase_with_leading_article(wearing)
            ));
        }
        (false, false) => {
            sentence.push_str(&format!(
                " carrying {} and wearing {}.",
                phrase_with_leading_article(holding),
                phrase_with_leading_article(wearing)
            ));
        }
    }
    sentence
}

fn format_weight_clause(player: &Object, objects: &HashMap<ObjectId, Object>) -> String {
    let carried = player_carried_weight(player, objects);
    match player_effective_max_weight(player, objects) {
        Some(max) if is_unlimited_weight(max) => format!(
            "are carrying {}/unlimited weight.",
            format_weight_amount(carried)
        ),
        Some(max) => format!(
            "are carrying {} of {} weight.",
            format_weight_amount(carried),
            format_weight_amount(max as f64)
        ),
        None => format!("are carrying {} weight.", format_weight_amount(carried)),
    }
}

fn format_capacity_and_weight(
    player: &Object,
    objects: &HashMap<ObjectId, Object>,
    plan: &BodyPlan,
) -> String {
    let occupied = occupied_body_slots(player, plan);
    let total = plan.slots.len() as u32;
    format!(
        "You have a carry capacity of {occupied}/{total} and {}",
        format_weight_clause(player, objects)
    )
}

/// Player self-examination (`examine self`).
///
/// Example:
/// ```text
/// You're a human carrying a Rusty Sword and wearing a backpack. You have a carry capacity of 2/10 and are carrying 13 of 100 weight.
/// ```
pub fn format_examine_self(
    player: &Object,
    objects: &HashMap<ObjectId, Object>,
    anatomy: &AnatomyRegistry,
) -> String {
    let creature = player
        .body_plan_name()
        .unwrap_or_else(|| "human".to_string());

    let Some(plan) = anatomy.body_plan(&creature) else {
        let identity = format_identity_sentence(&creature, &[], &[]);
        let health = crate::creature::format_health_clause(player, Some(anatomy));
        return format!(
            "{identity} {health} {}",
            format_weight_clause(player, objects)
        );
    };

    let (holding, wearing) = collect_gear_lists(player, objects, plan);
    let identity = format_identity_sentence(&creature, &holding, &wearing);
    let capacity = format_capacity_and_weight(player, objects, plan);
    let health = crate::creature::format_health_clause(player, Some(anatomy));
    let vitals = crate::creature::format_creature_stats_summary(player);
    if vitals.is_empty() {
        format!("{identity} {health} {capacity}")
    } else {
        format!("{identity} {health} You are {vitals}. {capacity}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mudl::load_module;
    use crate::object::{ContainerSpec, PermissionFlags};

    fn anatomy() -> AnatomyRegistry {
        load_module("modules/default")
            .unwrap()
            .active_world()
            .unwrap()
            .anatomy
            .clone()
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
            is_deleted: false,
            deleted_at: None,
        }
    }

    #[test]
    fn examine_self_natural_gear_and_stats() {
        let anatomy = anatomy();
        let mut player = bare("player:hero-001", "Admin");
        player.init_creature_role(anatomy.player_template("default").unwrap());
        crate::creature::init_creature_vitality(
            &mut player,
            anatomy.creature("human").expect("human"),
        );
        player.set_property_int("max_weight", 100);

        let mut rusty = bare("item:rusty-001", "Rusty Sword");
        rusty.set_property_string("hand_slot", "right");

        let mut wooden = bare("item:wooden-001", "Wooden Sword");
        wooden.set_property_string("hand_slot", "left");

        let mut backpack = bare("item:backpack-001", "backpack");
        backpack.apply_container_role(&ContainerSpec {
            capacity: 5,
            max_weight: None,
            max_volume: None,
            wearable: true,
            wear_slot: Some("torso".to_string()),
            ..crate::object::ContainerSpec::default()
        });

        player.set_body_slot("right_hand", Some(rusty.id.clone()));
        player.set_body_slot("left_hand", Some(wooden.id.clone()));
        player.set_body_slot("torso", Some(backpack.id.clone()));

        let mut objects = HashMap::new();
        objects.insert(rusty.id.clone(), rusty);
        objects.insert(wooden.id.clone(), wooden);
        objects.insert(backpack.id.clone(), backpack);
        objects.insert(player.id.clone(), player.clone());

        let output = format_examine_self(&player, &objects, &anatomy);
        assert!(output.starts_with(
            "You're a human carrying a Rusty Sword and Wooden Sword and wearing a backpack."
        ));
        assert!(output.contains("You feel fit."));
        assert!(output.contains("Strength 10"));
        assert!(output.contains("carry capacity of 3/10"));
        assert!(!output.contains("Admin"));
    }

    #[test]
    fn examine_self_empty_equipment() {
        let anatomy = anatomy();
        let mut player = bare("player:hero-001", "Admin");
        player.init_creature_role(anatomy.player_template("default").unwrap());
        crate::creature::init_creature_vitality(
            &mut player,
            anatomy.creature("human").expect("human"),
        );
        player.set_property_int("max_weight", 100);

        let output = format_examine_self(&player, &HashMap::new(), &anatomy);
        assert!(output.starts_with("You're a human."));
        assert!(output.contains("You feel fit."));
        assert!(output.contains("carry capacity of 0/10"));
    }
}
