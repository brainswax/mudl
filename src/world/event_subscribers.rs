//! Event bus subscribers — spawners, loot, and resources react through `execute_event`.

use std::collections::HashMap;

use crate::creature::spawner::{dispatch_creature_spawners_for_event, spawners_in_room};
use crate::loot::spawner::dispatch_loot_spawners_for_event;
use crate::mudl::AnatomyRegistry;
use crate::mudl::trigger_def::events;
use crate::object::ObjectId;
use crate::resource::spawner::dispatch_resource_spawners_for_event;

use super::events::{EventContext, EventOutcome};
use super::scheduler::advance_tick;

fn actor_owner(ctx: &EventContext, objects: &HashMap<ObjectId, crate::object::Object>) -> ObjectId {
    objects
        .get(&ctx.actor_id)
        .map(|actor| actor.owner.clone())
        .unwrap_or_else(|| ctx.actor_id.clone())
}

/// Run spawner/loot/resource modules subscribed to `event_name` on `ctx.host_id`.
pub fn dispatch_event_subscribers(
    event_name: &str,
    ctx: &EventContext,
    objects: &mut HashMap<ObjectId, crate::object::Object>,
    anatomy: Option<&AnatomyRegistry>,
) -> EventOutcome {
    let owner = actor_owner(ctx, objects);
    let mut outcome = EventOutcome::default();

    let scheduler_tick = if event_name == events::ON_ENTER {
        let tick = advance_tick(&ctx.host_id, events::ON_ENTER, objects);
        outcome.mark_dirty(&ctx.host_id);
        Some(tick)
    } else {
        None
    };

    if event_name == events::ON_ENTER {
        let anatomy = anatomy.cloned().unwrap_or_default();
        outcome.append(dispatch_creature_spawners_for_event(
            event_name,
            &ctx.host_id,
            &ctx.actor_id,
            &owner,
            &anatomy,
            objects,
            scheduler_tick,
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
        scheduler_tick,
    ));

    outcome.append(dispatch_resource_spawners_for_event(
        event_name,
        &ctx.host_id,
        &ctx.actor_id,
        &owner,
        objects,
        scheduler_tick,
    ));

    outcome
}