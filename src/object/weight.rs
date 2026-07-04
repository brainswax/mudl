//! Recursive weight calculations with cycle protection.

use std::collections::{HashMap, HashSet};

use crate::object::{Object, ObjectId};

/// Format a weight for display: whole numbers without decimals, fractions to one decimal.
pub fn format_weight_amount(w: f64) -> String {
    if !w.is_finite() {
        return "0".to_string();
    }
    let rounded = (w * 10.0).round() / 10.0;
    if rounded.fract().abs() < 1e-9 {
        format!("{}", rounded.round() as i64)
    } else {
        format!("{:.1}", rounded)
    }
}

/// Default carrying capacity for new players.
pub const DEFAULT_PLAYER_MAX_WEIGHT: i64 = 100;

/// `max_weight` value meaning unlimited capacity.
pub const UNLIMITED_WEIGHT: i64 = -1;

/// Whether a weight limit value denotes unlimited capacity.
pub fn is_unlimited_weight(limit: i64) -> bool {
    limit < 0
}

/// Whether a stored weight limit should be enforced or displayed as a cap.
pub fn weight_limit_applies(limit: Option<i64>) -> bool {
    limit.is_some_and(|l| !is_unlimited_weight(l))
}

impl Object {
    /// Total weight of this object: own weight (including stack scaling) plus nested contents.
    pub fn total_weight(&self, objects: &HashMap<ObjectId, Object>) -> f64 {
        let mut visited = HashSet::new();
        self.total_weight_inner(objects, &mut visited)
    }

    fn total_weight_inner(
        &self,
        objects: &HashMap<ObjectId, Object>,
        visited: &mut HashSet<ObjectId>,
    ) -> f64 {
        if !visited.insert(self.id.clone()) {
            return 0.0;
        }

        let mut sum = self.weight();
        if self.is_container() {
            for id in self.container_contents() {
                if let Some(child) = objects.get(&id) {
                    sum += child.total_weight_inner(objects, visited);
                }
            }
        }
        sum
    }

    /// Sum of total weights of direct contents (recursive through nested containers).
    ///
    /// Used for container capacity checks and "current/max" display. Excludes this
    /// container's own shell weight.
    pub fn contents_weight(&self, objects: &HashMap<ObjectId, Object>) -> f64 {
        if !self.is_container() {
            return 0.0;
        }
        self.container_contents()
            .iter()
            .filter_map(|id| objects.get(id))
            .map(|child| child.total_weight(objects))
            .sum()
    }
}

/// Total weight carried by a player across body slots and nested containers.
pub fn player_carried_weight(player: &Object, objects: &HashMap<ObjectId, Object>) -> f64 {
    let mut total = 0.0;
    let mut seen = HashSet::new();
    for item_id in player.carried_body_items() {
        if !seen.insert(item_id.clone()) {
            continue;
        }
        let Some(item) = objects.get(&item_id) else {
            continue;
        };
        total += item.total_weight(objects);
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
    fn format_weight_amount_one_decimal_for_fractions() {
        assert_eq!(format_weight_amount(2.1), "2.1");
        assert_eq!(format_weight_amount(2.0), "2");
        assert_eq!(format_weight_amount(0.1), "0.1");
        assert_eq!(format_weight_amount(21.0), "21");
    }

    #[test]
    fn stackable_fractional_total_weight() {
        let mut coins = bare("item:coins-001");
        coins.name = "coins".to_string();
        coins.set_property_numeric("weight", 0.1);
        coins.apply_stackable_role(&StackableSpec {
            count: 21,
            max_stack: 99,
        });
        assert!((coins.weight() - 2.1).abs() < f64::EPSILON);
        assert_eq!(format_weight_amount(coins.weight()), "2.1");
    }

    #[test]
    fn weight_limit_applies_excludes_unlimited() {
        assert!(!weight_limit_applies(Some(UNLIMITED_WEIGHT)));
        assert!(weight_limit_applies(Some(10)));
        assert!(!weight_limit_applies(None));
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
        assert!((coins.total_weight(&HashMap::new()) - 20.0).abs() < f64::EPSILON);
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

        assert!((pouch.total_weight(&objects) - 6.0).abs() < f64::EPSILON);
        assert!((purse.contents_weight(&objects) - 6.0).abs() < f64::EPSILON);
        assert!((purse.total_weight(&objects) - 7.0).abs() < f64::EPSILON);
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
        assert!(w >= 1.0);
        assert!(w < 1000.0);
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
        assert!((player_carried_weight(&player, &objects) - 21.0).abs() < f64::EPSILON);
    }
}