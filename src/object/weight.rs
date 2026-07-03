//! Recursive weight calculations with cycle protection.

use std::collections::{HashMap, HashSet};

use crate::object::{Object, ObjectId};

impl Object {
    /// Total weight of this object: own weight (including stack scaling) plus nested contents.
    pub fn total_weight(&self, objects: &HashMap<ObjectId, Object>) -> i64 {
        let mut visited = HashSet::new();
        self.total_weight_inner(objects, &mut visited)
    }

    fn total_weight_inner(
        &self,
        objects: &HashMap<ObjectId, Object>,
        visited: &mut HashSet<ObjectId>,
    ) -> i64 {
        if !visited.insert(self.id.clone()) {
            return 0;
        }

        let mut sum = self.weight();
        if self.is_container() {
            for id in self.container_contents() {
                if let Some(child) = objects.get(&id) {
                    sum = sum.saturating_add(child.total_weight_inner(objects, visited));
                }
            }
        }
        sum
    }

    /// Sum of total weights of direct contents (recursive through nested containers).
    ///
    /// Used for container capacity checks and "current/max" display. Excludes this
    /// container's own shell weight.
    pub fn contents_weight(&self, objects: &HashMap<ObjectId, Object>) -> i64 {
        if !self.is_container() {
            return 0;
        }
        self.container_contents()
            .iter()
            .filter_map(|id| objects.get(id))
            .map(|child| child.total_weight(objects))
            .sum()
    }
}

/// Total weight carried by a player across body slots and nested containers.
pub fn player_carried_weight(player: &Object, objects: &HashMap<ObjectId, Object>) -> i64 {
    let mut total = 0i64;
    let mut seen = HashSet::new();
    for item_id in player.carried_body_items() {
        if !seen.insert(item_id.clone()) {
            continue;
        }
        let Some(item) = objects.get(&item_id) else {
            continue;
        };
        total = total.saturating_add(item.total_weight(objects));
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{ContainerSpec, PermissionFlags, StackableSpec};

    fn bare(id: &str) -> Object {
        Object {
            id: ObjectId::new(id),
            name: "item".to_string(),
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
    fn stackable_total_weight_scales_with_count() {
        let mut coins = bare("item:coins-001");
        coins.name = "coins".to_string();
        coins.set_property_int("weight", 2);
        coins.apply_stackable_role(&StackableSpec {
            count: 10,
            max_stack: 99,
        });
        assert_eq!(coins.total_weight(&HashMap::new()), 20);
    }

    #[test]
    fn nested_container_weight_is_recursive() {
        let mut purse = bare("item:purse-001");
        purse.name = "purse".to_string();
        purse.set_property_int("weight", 1);
        purse.apply_container_role(&ContainerSpec {
            capacity: 3,
            max_weight: Some(10),
            max_volume: None,
            wearable: true,
            wear_slot: Some("torso".to_string()),
        });

        let mut pouch = bare("item:pouch-001");
        pouch.name = "pouch".to_string();
        pouch.set_property_int("weight", 1);
        pouch.apply_container_role(&ContainerSpec {
            capacity: 2,
            max_weight: None,
            max_volume: None,
            wearable: false,
            wear_slot: None,
        });

        let mut coins = bare("item:coins-001");
        coins.name = "coins".to_string();
        coins.set_property_int("weight", 1);
        coins.apply_stackable_role(&StackableSpec {
            count: 5,
            max_stack: 99,
        });

        pouch.set_property_list("contents", vec![coins.id.clone()]);
        purse.set_property_list("contents", vec![pouch.id.clone()]);

        let mut objects = HashMap::new();
        objects.insert(coins.id.clone(), coins);
        objects.insert(pouch.id.clone(), pouch.clone());
        objects.insert(purse.id.clone(), purse.clone());

        assert_eq!(pouch.total_weight(&objects), 6); // shell 1 + coins 5
        assert_eq!(purse.contents_weight(&objects), 6);
        assert_eq!(purse.total_weight(&objects), 7); // shell 1 + pouch tree 6
    }

    #[test]
    fn cycle_protection_avoids_infinite_recursion() {
        let mut a = bare("item:a-001");
        a.apply_container_role(&ContainerSpec::default());
        let mut b = bare("item:b-001");
        b.apply_container_role(&ContainerSpec::default());

        a.set_property_list("contents", vec![b.id.clone()]);
        b.set_property_list("contents", vec![a.id.clone()]);
        a.set_property_int("weight", 1);
        b.set_property_int("weight", 1);

        let mut objects = HashMap::new();
        objects.insert(a.id.clone(), a.clone());
        objects.insert(b.id.clone(), b.clone());

        let w = a.total_weight(&objects);
        assert!(w >= 1);
        assert!(w < 1000);
    }

    #[test]
    fn player_carried_weight_sums_nested_possession() {
        let mut player = bare("player:hero-001");
        player.name = "Hero".to_string();

        let mut purse = bare("item:purse-001");
        purse.set_property_int("weight", 1);
        purse.apply_container_role(&ContainerSpec {
            capacity: 3,
            max_weight: Some(10),
            max_volume: None,
            wearable: true,
            wear_slot: Some("torso".to_string()),
        });

        let mut coins = bare("item:coins-001");
        coins.set_property_int("weight", 1);
        coins.apply_stackable_role(&StackableSpec {
            count: 10,
            max_stack: 99,
        });
        purse.set_property_list("contents", vec![coins.id.clone()]);

        player.set_body_slot("torso", Some(purse.id.clone()));
        player.set_body_slot("right_hand", Some(coins.id.clone()));

        let mut objects = HashMap::new();
        objects.insert(player.id.clone(), player.clone());
        objects.insert(purse.id.clone(), purse);
        objects.insert(coins.id.clone(), coins);

        // Purse on torso (1 shell + 10 coins inside) + 10 coins in hand
        assert_eq!(player_carried_weight(&player, &objects), 21);
    }
}