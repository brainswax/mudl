//! Event bus subscribers — scheduler, spawners, and loot/resources react through `execute_event`.

use std::collections::HashMap;

use crate::creature::spawner::{dispatch_creature_spawners_for_event, spawners_in_room};
use crate::loot::spawner::dispatch_loot_spawners_for_event;
use crate::mudl::AnatomyRegistry;
use crate::mudl::trigger_def::events;
use crate::object::{Object, ObjectId};
use crate::resource::spawner::dispatch_resource_spawners_for_event;

use super::event_script::execute_host_event;
use super::events::{EventContext, EventOutcome};
use super::scheduler::{advance_tick, due_schedule_jobs};

fn actor_owner(ctx: &EventContext, objects: &HashMap<ObjectId, crate::object::Object>) -> ObjectId {
    objects
        .get(&ctx.actor_id)
        .map(|actor| actor.owner.clone())
        .unwrap_or_else(|| ctx.actor_id.clone())
}

fn append_subscriber(
    label: &str,
    outcome: &mut EventOutcome,
    dispatch: impl FnOnce() -> EventOutcome,
) {
    if outcome.is_cancelled() {
        return;
    }
    let result = dispatch();
    if !result.errors.is_empty() {
        for error in &result.errors {
            outcome.record_error(format!("{label}: {error}"));
        }
    }
    outcome.append(result);
}

/// Fire `@schedule` jobs due on this room-enter tick.
fn dispatch_scheduled_events(
    tick: u64,
    ctx: &EventContext,
    objects: &mut HashMap<ObjectId, Object>,
    anatomy: Option<&AnatomyRegistry>,
) -> EventOutcome {
    let mut outcome = EventOutcome::default();
    let due = due_schedule_jobs(&ctx.host_id, tick, objects);
    let due_count = due.len();
    for (event_name, host_id) in due {
        if outcome.is_cancelled() {
            break;
        }
        let job_ctx = EventContext {
            actor_id: ctx.actor_id.clone(),
            host_id: host_id.clone(),
            room_id: ctx.room_id.clone().or(Some(ctx.host_id.clone())),
            target_id: ctx.target_id.clone(),
        };
        // Scheduled jobs run host `@trigger` scripts only — no subscriber re-entry.
        outcome.append(execute_host_event(
            &event_name,
            &job_ctx,
            objects,
            anatomy,
        ));
        outcome.mark_dirty(&host_id);
    }
    if due_count > 0 {
        outcome.mark_dirty(&ctx.host_id);
    }
    outcome
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
        append_subscriber("scheduler", &mut outcome, || {
            dispatch_scheduled_events(tick, ctx, objects, anatomy)
        });
        Some(tick)
    } else {
        None
    };

    if event_name == events::ON_ENTER {
        if let Some(anatomy) = anatomy {
            append_subscriber("creature_spawner", &mut outcome, || {
                dispatch_creature_spawners_for_event(
                    event_name,
                    &ctx.host_id,
                    &ctx.actor_id,
                    &owner,
                    anatomy,
                    objects,
                    scheduler_tick,
                )
            });
        }
        if !outcome.is_cancelled() {
            for spawner in spawners_in_room(&ctx.host_id, objects) {
                outcome.mark_dirty(&spawner.id);
            }
        }
    }

    append_subscriber("loot_spawner", &mut outcome, || {
        dispatch_loot_spawners_for_event(
            event_name,
            &ctx.host_id,
            &ctx.actor_id,
            &owner,
            objects,
            scheduler_tick,
        )
    });

    append_subscriber("resource_spawner", &mut outcome, || {
        dispatch_resource_spawners_for_event(
            event_name,
            &ctx.host_id,
            &ctx.actor_id,
            &owner,
            objects,
            scheduler_tick,
        )
    });

    outcome
}