//! Event bus subscribers — creature and loot spawners react through `execute_event`.

use std::collections::HashMap;

use crate::creature::spawner::{dispatch_creature_spawners_for_event, spawners_in_room};
use crate::loot::spawner::dispatch_loot_spawners_for_event;
use crate::mudl::AnatomyRegistry;
use crate::mudl::trigger_def::events;
use crate::object::ObjectId;

use super::events::{EventContext, EventOutcome};

fn actor_owner(ctx: &EventContext, objects: &HashMap<ObjectId, crate::object::Object>) -> ObjectId {
    objects
        .get(&ctx.actor_id)
        .map(|actor| actor.owner.clone())
        .unwrap_or_else(|| ctx.actor_id.clone())
}

/// Run spawner/loot modules subscribed to `event_name` on `ctx.host_id`.
pub fn dispatch_event_subscribers(
    event_name: &str,
    ctx: &EventContext,
    objects: &mut HashMap<ObjectId, crate::object::Object>,
    anatomy: Option<&AnatomyRegistry>,
) -> EventOutcome {
    let owner = actor_owner(ctx, objects);
    let mut outcome = EventOutcome::default();

    if event_name == events::ON_ENTER {
        let anatomy = anatomy.cloned().unwrap_or_default();
        outcome.append(dispatch_creature_spawners_for_event(
            event_name,
            &ctx.host_id,
            &ctx.actor_id,
            &owner,
            &anatomy,
            objects,
        ));
        for spawner in spawners_in_room(&ctx.host_id, objects) {
            outcome.mark_dirty(&spawner.id);
        }
    }

    outcome.append(dispatch_loot_spawners_for_event(
        event_name,
        &ctx.host_id,
        &ctx.actor_id,
        &owner,
        objects,
    ));

    outcome
}