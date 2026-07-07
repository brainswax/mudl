//! Player-facing examine output for creatures (NPCs).

use std::collections::HashMap;

use crate::mudl::AnatomyRegistry;
use crate::object::{Object, ObjectId};

/// In-character `examine` for an NPC or other non-player creature.
pub fn format_examine_creature_player(
    obj: &Object,
    objects: &HashMap<ObjectId, Object>,
    anatomy: &AnatomyRegistry,
) -> String {
    let mut lines = Vec::new();
    if let Some(creature) = obj.creature_name() {
        lines.push(format!("A {} named {}.", creature, obj.name.to_lowercase()));
    } else {
        lines.push(format!("A creature named {}.", obj.name.to_lowercase()));
    }
    lines.push(crate::creature::format_npc_health_clause(
        obj,
        Some(anatomy),
    ));
    let gauge = crate::creature::format_creature_gauge(
        obj,
        |name| crate::creature::creature_effective_stat(obj, name, objects, anatomy),
        |name| crate::creature::creature_effective_skill(obj, name, objects, anatomy),
    );
    if !gauge.is_empty() {
        lines.push(format!("You gauge: {gauge}."));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::vitality::init_creature_vitality;
    use crate::mudl::load_module;
    use crate::mudl::PlayerTemplate;
    use crate::object::{ObjectId, PermissionFlags};
    use std::collections::HashMap;

    #[test]
    fn examine_npc_shows_health_and_stats() {
        let anatomy = load_module("modules/default")
            .unwrap()
            .active_world()
            .unwrap()
            .anatomy
            .clone();
        let human = anatomy.creature("human").unwrap();
        let mut watcher = Object {
            id: ObjectId::new("npc:watcher-001"),
            name: "Path Watcher".to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: ObjectId::new("player:admin-001"),
            permissions: PermissionFlags::EVERYONE,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        watcher.init_creature_role(&PlayerTemplate {
            name: "watcher".to_string(),
            creature: "human".to_string(),
            gender: "neutral".to_string(),
        });
        init_creature_vitality(&mut watcher, human);

        let output = format_examine_creature_player(&watcher, &HashMap::new(), &anatomy);
        assert!(output.contains("path watcher"));
        assert!(output.contains("looks fit"));
        assert!(output.contains("Strength 10"));
    }
}
