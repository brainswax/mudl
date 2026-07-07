//! Central event scheduler — scope ticks, property counters, and timed jobs.

use std::collections::HashMap;

use crate::mudl::schedule_def::ScheduleDef;
use crate::object::{Object, ObjectId, PermissionFlags, Property, Value};

const JOBS_PROPERTY: &str = "scheduler_jobs";
const COUNTERS_PROPERTY: &str = "scheduler_counters";

fn tick_property(event: &str) -> String {
    format!("scheduler_tick_{event}")
}

/// Read the current tick for `scope_id` + `event` without advancing.
pub fn current_tick(
    scope_id: &ObjectId,
    event: &str,
    objects: &HashMap<ObjectId, Object>,
) -> u64 {
    objects
        .get(scope_id)
        .and_then(|scope| scope.get_int_property(&tick_property(event)))
        .unwrap_or(0)
        .max(0) as u64
}

/// Advance and return the tick for `scope_id` + `event`.
pub fn advance_tick(
    scope_id: &ObjectId,
    event: &str,
    objects: &mut HashMap<ObjectId, Object>,
) -> u64 {
    let tick = current_tick(scope_id, event, objects) + 1;
    if let Some(scope) = objects.get_mut(scope_id) {
        scope.set_property_int(&tick_property(event), tick as i64);
    }
    tick
}

/// Whether a periodic subscriber should fire on this `tick` (every `interval` ticks).
pub fn periodic_fires(tick: u64, interval: u32) -> bool {
    let interval = u64::from(interval.max(1));
    tick > 0 && tick.is_multiple_of(interval)
}

/// Read a named counter on `scope_id`.
pub fn read_counter(
    scope_id: &ObjectId,
    name: &str,
    objects: &HashMap<ObjectId, Object>,
) -> u64 {
    objects
        .get(scope_id)
        .and_then(|scope| scope.get_property(COUNTERS_PROPERTY))
        .and_then(|prop| counter_map(&prop.value))
        .and_then(|map| map.get(name).copied())
        .unwrap_or(0)
        .max(0) as u64
}

/// Increment a named counter on `scope_id` and return the new value.
pub fn increment_counter(
    scope_id: &ObjectId,
    name: &str,
    objects: &mut HashMap<ObjectId, Object>,
) -> u64 {
    let next = read_counter(scope_id, name, objects) + 1;
    if let Some(scope) = objects.get_mut(scope_id) {
        let mut map = scope
            .get_property(COUNTERS_PROPERTY)
            .and_then(|prop| counter_map(&prop.value))
            .unwrap_or_default();
        map.insert(name.to_string(), next as i64);
        scope.set_int_map(COUNTERS_PROPERTY, map);
    }
    next
}

/// Reset a named counter on `scope_id`.
pub fn reset_counter(
    scope_id: &ObjectId,
    name: &str,
    objects: &mut HashMap<ObjectId, Object>,
) {
    if let Some(scope) = objects.get_mut(scope_id) {
        let mut map = scope
            .get_property(COUNTERS_PROPERTY)
            .and_then(|prop| counter_map(&prop.value))
            .unwrap_or_default();
        map.remove(name);
        if map.is_empty() {
            scope.properties.remove(COUNTERS_PROPERTY);
        } else {
            scope.set_int_map(COUNTERS_PROPERTY, map);
        }
    }
}

fn counter_map(value: &Value) -> Option<HashMap<String, i64>> {
    let Value::Map(map) = value else {
        return None;
    };
    Some(
        map.iter()
            .filter_map(|(key, value)| match value {
                Value::Int(n) if *n >= 0 => Some((key.clone(), *n)),
                _ => None,
            })
            .collect(),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ScheduleJob {
    id: String,
    event: String,
    interval: u32,
    host_id: ObjectId,
}

fn parse_jobs(obj: &Object) -> Vec<ScheduleJob> {
    obj.get_property(JOBS_PROPERTY)
        .and_then(|prop| {
            let Value::List(items) = &prop.value else {
                return None;
            };
            Some(
                items
                    .iter()
                    .filter_map(|entry| {
                        let Value::Map(map) = entry else {
                            return None;
                        };
                        let id = map.get("id").and_then(|v| {
                            if let Value::String(s) = v {
                                Some(s.clone())
                            } else {
                                None
                            }
                        })?;
                        let event = map.get("event").and_then(|v| {
                            if let Value::String(s) = v {
                                Some(s.clone())
                            } else {
                                None
                            }
                        })?;
                        let interval = map
                            .get("interval")
                            .and_then(|v| match v {
                                Value::Int(n) => Some(*n as u32),
                                _ => None,
                            })
                            .unwrap_or(1)
                            .max(1);
                        let host_id = map.get("host").and_then(|v| {
                            if let Value::ObjectRef(id) = v {
                                Some(id.clone())
                            } else {
                                None
                            }
                        })?;
                        Some(ScheduleJob {
                            id,
                            event,
                            interval,
                            host_id,
                        })
                    })
                    .collect(),
            )
        })
        .unwrap_or_default()
}

fn jobs_to_values(jobs: &[ScheduleJob]) -> Vec<Value> {
    jobs.iter()
        .map(|job| {
            Value::Map(HashMap::from([
                ("id".to_string(), Value::String(job.id.clone())),
                ("event".to_string(), Value::String(job.event.clone())),
                (
                    "interval".to_string(),
                    Value::Int(i64::from(job.interval.max(1))),
                ),
                ("host".to_string(), Value::ObjectRef(job.host_id.clone())),
            ]))
        })
        .collect()
}

/// Register a MUDL schedule job on `scope` (usually the schedule target itself).
pub fn register_schedule_job(scope: &mut Object, def: &ScheduleDef, host_id: &ObjectId) {
    let mut jobs = parse_jobs(scope);
    if jobs.iter().any(|job| job.id == def.base_name) {
        return;
    }
    jobs.push(ScheduleJob {
        id: def.base_name.clone(),
        event: def.event.clone(),
        interval: def.interval.max(1),
        host_id: host_id.clone(),
    });
    scope.add_property(Property {
        name: JOBS_PROPERTY.to_string(),
        value: Value::List(jobs_to_values(&jobs)),
        permissions: PermissionFlags::EVERYONE,
        behavior: None,
    });
}

/// Jobs registered on `scope_id` that are due on `tick`.
pub fn due_schedule_jobs(
    scope_id: &ObjectId,
    tick: u64,
    objects: &HashMap<ObjectId, Object>,
) -> Vec<(String, ObjectId)> {
    objects
        .get(scope_id)
        .map(parse_jobs)
        .unwrap_or_default()
        .into_iter()
        .filter(|job| {
            periodic_fires(tick, job.interval)
                && objects
                    .get(&job.host_id)
                    .is_some_and(|host| host.is_active())
        })
        .map(|job| (job.event, job.host_id))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_scope(id: &str) -> Object {
        Object {
            id: ObjectId::new(id),
            name: "Test Scope".to_string(),
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
        }
    }

    #[test]
    fn advance_and_periodic_interval() {
        let room_id = ObjectId::new("area:test-001");
        let mut objects = HashMap::from([(room_id.clone(), sample_scope("area:test-001"))]);

        assert_eq!(advance_tick(&room_id, "on_enter", &mut objects), 1);
        assert_eq!(advance_tick(&room_id, "on_enter", &mut objects), 2);
        assert_eq!(current_tick(&room_id, "on_enter", &objects), 2);
        assert!(!periodic_fires(1, 3));
        assert!(!periodic_fires(2, 3));
        assert!(periodic_fires(3, 3));
    }

    #[test]
    fn property_counters_increment_and_reset() {
        let scope_id = ObjectId::new("area:test-001");
        let mut objects = HashMap::from([(scope_id.clone(), sample_scope("area:test-001"))]);

        assert_eq!(read_counter(&scope_id, "visits", &objects), 0);
        assert_eq!(increment_counter(&scope_id, "visits", &mut objects), 1);
        assert_eq!(increment_counter(&scope_id, "visits", &mut objects), 2);
        reset_counter(&scope_id, "visits", &mut objects);
        assert_eq!(read_counter(&scope_id, "visits", &objects), 0);
    }

    #[test]
    fn due_schedule_jobs_skips_inactive_host() {
        let room_id = ObjectId::new("area:mist-001");
        let mut room = sample_scope("area:mist-001");
        register_schedule_job(
            &mut room,
            &ScheduleDef {
                base_name: "mist-weather".to_string(),
                target: "haunted-mist".to_string(),
                interval: 1,
                event: "on_weather".to_string(),
            },
            &ObjectId::new("area:gone-001"),
        );
        let objects = HashMap::from([(room_id.clone(), room)]);

        assert!(due_schedule_jobs(&room_id, 1, &objects).is_empty());
    }

    #[test]
    fn register_schedule_job_is_idempotent() {
        let room_id = ObjectId::new("area:test-001");
        let mut room = sample_scope("area:test-001");
        let def = ScheduleDef {
            base_name: "mist-weather".to_string(),
            target: "haunted-mist".to_string(),
            interval: 2,
            event: "on_weather".to_string(),
        };
        register_schedule_job(&mut room, &def, &room_id);
        register_schedule_job(&mut room, &def, &room_id);
        let jobs = parse_jobs(&room);
        assert_eq!(jobs.len(), 1);
    }

    #[test]
    fn due_schedule_jobs_respects_interval() {
        let room_id = ObjectId::new("area:mist-001");
        let mut room = sample_scope("area:mist-001");
        register_schedule_job(
            &mut room,
            &ScheduleDef {
                base_name: "mist-weather".to_string(),
                target: "haunted-mist".to_string(),
                interval: 2,
                event: "on_weather".to_string(),
            },
            &room_id,
        );
        let objects = HashMap::from([(room_id.clone(), room)]);

        assert!(due_schedule_jobs(&room_id, 1, &objects).is_empty());
        let due = due_schedule_jobs(&room_id, 2, &objects);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].0, "on_weather");
    }
}