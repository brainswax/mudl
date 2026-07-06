//! Back-compat re-exports — portals supersede the door-only module.

pub use super::portal::{
    door_for_direction, door_passage_block, door_permits_exit, portal_for_direction,
    portal_kind_label, portal_passage_block, portal_permits_exit, portals_in_room, DoorBlock,
    PortalBlock,
};