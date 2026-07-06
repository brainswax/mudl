//! Body plan (creature anatomy) formatting for examine and look.

use crate::mudl::{AnatomyRegistry, BodyPlan, SlotType};
use crate::object::Object;

use super::self_examine::equipment_placement_label;

/// Ordered friendly slot names for body detail views.
pub fn format_available_slots(plan: &BodyPlan) -> String {
    let names: Vec<String> = plan
        .slots
        .iter()
        .map(|s| equipment_placement_label(&s.name))
        .collect();
    names.join(", ")
}

/// Detailed body anatomy (`examine self body`, `examine human`).
///
/// Example: `You are human. Available slots: head, back, right hand, left hand, ...`
pub fn format_body_detail_player(plan: &BodyPlan, addressing_self: bool) -> String {
    let slots = format_available_slots(plan);
    if addressing_self {
        format!(
            "You are {}. Available slots: {}.",
            plan.name, slots
        )
    } else {
        format!(
            "{} anatomy. Available slots: {}.",
            capitalize_first(&plan.name),
            slots
        )
    }
}

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

/// Player-facing examine output for a creature definition by name (`examine human`).
pub fn format_body_plan_examine_player(plan: &BodyPlan, _carry_capacity: Option<f64>) -> String {
    format_body_detail_player(plan, false)
}

/// Builder `@examine` section listing slot definitions and occupancy.
pub fn format_anatomy_section(
    player: &Object,
    plan: &BodyPlan,
    objects: &std::collections::HashMap<crate::object::ObjectId, Object>,
) -> Vec<String> {
    let occupied = player.body_slots();
    plan.slots
        .iter()
        .map(|slot| {
            let type_label = match slot.slot_type {
                SlotType::Grasp => "grasp",
                SlotType::Wear => "wear",
                SlotType::Limb => "limb",
                SlotType::Pocket => "pocket",
                SlotType::Container => "container",
            };
            let occupant = occupied
                .get(&slot.name)
                .and_then(|id| objects.get(id))
                .map(|obj| obj.name.to_lowercase())
                .unwrap_or_else(|| "empty".to_string());
            format!(
                "{} ({}, capacity {}): {}",
                slot.name,
                type_label,
                slot.capacity,
                occupant
            )
        })
        .collect()
}

/// Builder `@examine` for a creature definition by name.
pub fn format_body_plan_examine_builder(plan: &BodyPlan) -> String {
    let mut lines = vec![
        format!("name: {}", plan.name),
        "type: body_plan".to_string(),
        "slots:".to_string(),
    ];
    for slot in &plan.slots {
        let type_label = match slot.slot_type {
            SlotType::Grasp => "grasp",
            SlotType::Wear => "wear",
            SlotType::Limb => "limb",
            SlotType::Pocket => "pocket",
            SlotType::Container => "container",
        };
        lines.push(format!(
            "  {}: type={}, capacity={}, hands={}",
            slot.name, type_label, slot.capacity, slot.hands
        ));
    }
    lines.join("\n")
}

/// Resolve a creature name against loaded anatomy (for `examine human`).
pub fn creature_definition<'a>(
    name: &str,
    anatomy: &'a AnatomyRegistry,
) -> Option<&'a BodyPlan> {
    anatomy.body_plan(&name.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mudl::load_module;

    fn human_plan() -> BodyPlan {
        load_module("modules/default")
            .unwrap()
            .active_world()
            .unwrap()
            .anatomy
            .body_plan("human")
            .unwrap()
            .clone()
    }

    #[test]
    fn body_detail_lists_available_slots() {
        let plan = human_plan();
        let output = format_body_detail_player(&plan, true);
        assert!(output.starts_with("You are human."));
        assert!(output.contains("Available slots:"));
        assert!(output.contains("right hand"));
        assert!(output.contains("left hand"));
        assert!(output.contains("back"));
    }

    #[test]
    fn body_plan_examine_creature_uses_anatomy_heading() {
        let plan = human_plan();
        let output = format_body_plan_examine_player(&plan, None);
        assert!(output.starts_with("Human anatomy."));
        assert!(output.contains("Available slots:"));
    }

    #[test]
    fn body_plan_examine_builder_shows_slot_types() {
        let plan = human_plan();
        let output = format_body_plan_examine_builder(&plan);
        assert!(output.contains("type: body_plan"));
        assert!(output.contains("left_hand: type=grasp"));
        assert!(output.contains("torso: type=wear"));
    }
}