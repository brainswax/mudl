//! Body plan (creature anatomy) formatting for examine and look.

use crate::mudl::{slot_display_name, AnatomyRegistry, BodyPlan, BodySlotDef, SlotType};
use crate::object::{format_weight_amount, Object};

/// Compact player-facing summary of a body plan.
///
/// Example: `Body (human): 2 grasp slots (left hand, right hand); wear: head, torso; carry up to 100 weight.`
pub fn format_body_plan_summary(plan: &BodyPlan, carry_capacity: Option<f64>) -> String {
    let mut parts = Vec::new();

    let grasp = plan.grasp_slots();
    if !grasp.is_empty() {
        let names: Vec<String> = grasp
            .iter()
            .map(|s| slot_display_name(&s.name))
            .collect();
        let label = if grasp.len() == 1 {
            "1 grasp slot".to_string()
        } else {
            format!("{} grasp slots", grasp.len())
        };
        parts.push(format!("{label} ({})", names.join(", ")));
    }

    let wear: Vec<String> = plan
        .wear_slots()
        .iter()
        .map(|s| slot_display_name(&s.name))
        .collect();
    if !wear.is_empty() {
        parts.push(format!("wear: {}", wear.join(", ")));
    }

    let limbs: Vec<String> = plan
        .slots_of_type(SlotType::Limb)
        .iter()
        .map(|s| slot_display_name(&s.name))
        .collect();
    if !limbs.is_empty() {
        parts.push(format!("limbs: {}", limbs.join(", ")));
    }

    let mut summary = format!("Body ({}): {}", plan.name, parts.join("; "));
    if let Some(cap) = carry_capacity {
        summary.push_str(&format!("; carry up to {} weight", format_weight_amount(cap)));
    }
    summary.push('.');
    summary
}

/// Player-facing examine output for a creature definition (e.g. `examine human`).
pub fn format_body_plan_examine_player(plan: &BodyPlan, carry_capacity: Option<f64>) -> String {
    let mut lines = vec![plan.name.clone()];
    lines.push(format_slot_group_player("Grasp", plan.grasp_slots()));
    lines.push(format_slot_group_player(
        "Wear",
        plan.wear_slots().into_iter().collect(),
    ));
    let limbs = plan.slots_of_type(SlotType::Limb);
    if !limbs.is_empty() {
        lines.push(format_slot_group_player("Limbs", limbs));
    }
    if let Some(cap) = carry_capacity {
        lines.push(format!(
            "Carry capacity: {} weight.",
            format_weight_amount(cap)
        ));
    }
    lines.join("\n")
}

fn format_slot_group_player(label: &str, slots: Vec<&BodySlotDef>) -> String {
    if slots.is_empty() {
        return format!("{label}: (none)");
    }
    let entries: Vec<String> = slots
        .iter()
        .map(|s| {
            if s.capacity > 1 {
                format!(
                    "{} (capacity {})",
                    slot_display_name(&s.name),
                    s.capacity
                )
            } else {
                slot_display_name(&s.name)
            }
        })
        .collect();
    format!("{label}: {}", entries.join(", "))
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
    fn body_plan_summary_lists_grasp_and_wear_slots() {
        let plan = human_plan();
        let summary = format_body_plan_summary(&plan, Some(100.0));
        assert!(summary.contains("Body (human)"));
        assert!(summary.contains("grasp"));
        assert!(summary.contains("left hand"));
        assert!(summary.contains("right hand"));
        assert!(summary.contains("wear: head, torso"));
        assert!(summary.contains("carry up to 100 weight"));
    }

    #[test]
    fn body_plan_examine_player_lists_slot_groups() {
        let plan = human_plan();
        let output = format_body_plan_examine_player(&plan, None);
        assert!(output.starts_with("human"));
        assert!(output.contains("Grasp: left hand, right hand"));
        assert!(output.contains("Wear: head, torso"));
        assert!(output.contains("Limbs: left arm, right arm"));
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