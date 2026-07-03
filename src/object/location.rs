//! Location references for the object graph (where an object resides).

use crate::object::ObjectId;

/// A typed reference to where an object can be located in the world graph.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LocationRef {
    /// On the ground in a room, area, or other navigable place.
    Room(ObjectId),
    /// Carried by a creature (player/NPC) — body slots and nested containers.
    Inventory(ObjectId),
    /// Inside a container object, optionally in a named sub-slot.
    Container(ObjectId, Option<String>),
    /// Worn or held in a specific anatomical slot on a creature.
    BodySlot(ObjectId, String),
    /// Not placed anywhere (abstract, deleted staging, etc.).
    Nowhere,
}

impl LocationRef {
    /// The object that directly holds this location, if any.
    pub fn holder_id(&self) -> Option<&ObjectId> {
        match self {
            Self::Room(id) | Self::Inventory(id) | Self::Container(id, _) => Some(id),
            Self::BodySlot(id, _) => Some(id),
            Self::Nowhere => None,
        }
    }

    /// Whether this location is a navigable place (room/area).
    pub fn is_room(&self) -> bool {
        matches!(self, Self::Room(_))
    }

    /// Whether this location is on a creature's person.
    pub fn is_inventory(&self) -> bool {
        matches!(self, Self::Inventory(_) | Self::BodySlot(_, _))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holder_id_for_container_includes_parent() {
        let bag = ObjectId::new("item:bag-001");
        let loc = LocationRef::Container(bag.clone(), Some("main".to_string()));
        assert_eq!(loc.holder_id(), Some(&bag));
    }

    #[test]
    fn nowhere_has_no_holder() {
        assert_eq!(LocationRef::Nowhere.holder_id(), None);
    }
}
