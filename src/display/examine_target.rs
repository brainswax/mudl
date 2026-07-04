//! Parsing and resolution for `examine` / `@examine` targets including parent/prototype views.

use std::collections::HashMap;

use crate::mudl::AnatomyRegistry;
use crate::object::{Object, ObjectId};

use super::body_plan::{
    creature_definition, format_body_plan_examine_builder, format_body_plan_examine_player,
};
use super::examine::{
    format_prototype_examine_builder, format_prototype_examine_player, PROTOTYPE_PROPERTY_KEYS,
};
use super::{
    resolve_object, Describable, DisplayContext, DisplayMode, ResolveScope, TargetResolution,
};

/// What the player or builder wants to inspect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExamineTarget {
    /// Normal object by name (`coins`, `self`, `here`).
    Object(String),
    /// Prototype/parent of an object (`coins.parent`, `@examine coins parent`, `#parent`).
    PrototypeOf(String),
}

/// Parsed examine request from command arguments (everything after the verb).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExamineRequest {
    /// No target — examine current room.
    Here,
    /// Resolved examine intent.
    Target(ExamineTarget),
    /// Creature anatomy definition (`examine human`).
    BodyPlan(String),
}

/// Result of resolving an examine request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExamineResolution {
    Object(ObjectId),
    Prototype {
        instance_id: ObjectId,
        prototype_id: ObjectId,
    },
    BodyPlan(String),
}

/// Failure modes specific to examine resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExamineError {
    Ambiguous(String),
    NotFound,
    NoParent(ObjectId),
}

/// Parse `examine` / `@examine` arguments into a request.
pub fn parse_examine_request(args: &[&str]) -> ExamineRequest {
    if args.is_empty() {
        return ExamineRequest::Here;
    }

    if args.len() == 1 {
        let token = args[0];
        if token.eq_ignore_ascii_case("#parent") {
            return ExamineRequest::Target(ExamineTarget::PrototypeOf("self".into()));
        }
        if let Some(base) = token.strip_suffix(".parent") {
            if !base.is_empty() {
                return ExamineRequest::Target(ExamineTarget::PrototypeOf(base.to_string()));
            }
        }
        return ExamineRequest::Target(ExamineTarget::Object(token.to_string()));
    }

    let aspect = args[1].to_ascii_lowercase();
    if aspect == "parent" || aspect == "#parent" {
        return ExamineRequest::Target(ExamineTarget::PrototypeOf(args[0].to_string()));
    }

    ExamineRequest::Target(ExamineTarget::Object(args.join(" ")))
}

/// If the name is a known creature definition and not a resolvable object, treat as body plan.
pub fn try_body_plan_name(
    name: &str,
    anatomy: &AnatomyRegistry,
    observer: &ObjectId,
    current_location: Option<&ObjectId>,
    objects: &HashMap<ObjectId, Object>,
) -> Option<String> {
    let needle = name.to_lowercase();
    if needle == "self" || needle == "me" || needle == "here" {
        return None;
    }
    if creature_definition(&needle, anatomy).is_none() {
        return None;
    }
    match resolve_object(
        name,
        observer,
        current_location,
        objects,
        ResolveScope::General,
    ) {
        TargetResolution::Found(_) => None,
        TargetResolution::Ambiguous(_) | TargetResolution::NotFound => Some(needle),
    }
}

/// Resolve an examine request to a concrete target.
pub fn resolve_examine_request(
    request: &ExamineRequest,
    anatomy: &AnatomyRegistry,
    observer: &ObjectId,
    current_location: Option<&ObjectId>,
    objects: &HashMap<ObjectId, Object>,
) -> Result<ExamineResolution, ExamineError> {
    match request {
        ExamineRequest::Here => {
            if let Some(loc) = current_location {
                Ok(ExamineResolution::Object(loc.clone()))
            } else {
                Err(ExamineError::NotFound)
            }
        }
        ExamineRequest::BodyPlan(name) => Ok(ExamineResolution::BodyPlan(name.clone())),
        ExamineRequest::Target(ExamineTarget::Object(name)) => {
            if let Some(plan_name) =
                try_body_plan_name(name, anatomy, observer, current_location, objects)
            {
                return Ok(ExamineResolution::BodyPlan(plan_name));
            }
            match resolve_object(
                name,
                observer,
                current_location,
                objects,
                ResolveScope::General,
            ) {
                TargetResolution::Found(id) => Ok(ExamineResolution::Object(id)),
                TargetResolution::Ambiguous(msg) => Err(ExamineError::Ambiguous(msg)),
                TargetResolution::NotFound => Err(ExamineError::NotFound),
            }
        }
        ExamineRequest::Target(ExamineTarget::PrototypeOf(name)) => {
            let lookup_name = if name.eq_ignore_ascii_case("self") || name.eq_ignore_ascii_case("me")
            {
                "self"
            } else {
                name.as_str()
            };
            let instance_id = match resolve_object(
                lookup_name,
                observer,
                current_location,
                objects,
                ResolveScope::General,
            ) {
                TargetResolution::Found(id) => id,
                TargetResolution::Ambiguous(msg) => return Err(ExamineError::Ambiguous(msg)),
                TargetResolution::NotFound => return Err(ExamineError::NotFound),
            };
            let instance = objects.get(&instance_id).ok_or(ExamineError::NotFound)?;
            if let Some(proto_id) = &instance.prototype {
                return Ok(ExamineResolution::Prototype {
                    instance_id,
                    prototype_id: proto_id.clone(),
                });
            }
            if instance.object_type() == "player" {
                if let Some(creature) = instance.creature_name() {
                    return Ok(ExamineResolution::BodyPlan(creature));
                }
            }
            Err(ExamineError::NoParent(instance_id))
        }
    }
}

/// Format examine output for a resolved request.
pub fn format_examine_output(
    resolution: &ExamineResolution,
    ctx: &DisplayContext,
) -> Option<String> {
    match resolution {
        ExamineResolution::Object(id) => ctx.objects.get(id).map(|obj| {
            if ctx.mode == DisplayMode::Builder {
                obj.describe_detailed(ctx)
            } else {
                obj.describe(ctx)
            }
        }),
        ExamineResolution::Prototype {
            instance_id,
            prototype_id,
        } => {
            let instance = ctx.objects.get(instance_id)?;
            let prototype = ctx.objects.get(prototype_id)?;
            Some(match ctx.mode {
                DisplayMode::Builder => {
                    format_prototype_examine_builder(instance, prototype, ctx)
                }
                _ => format_prototype_examine_player(instance, prototype, ctx),
            })
        }
        ExamineResolution::BodyPlan(name) => {
            let plan = ctx.anatomy.body_plan(name)?;
            let carry = ctx
                .objects
                .get(&ctx.observer)
                .and_then(|p| p.get_int_property("max_weight"))
                .map(|v| v as f64);
            Some(match ctx.mode {
                DisplayMode::Builder => format_body_plan_examine_builder(plan),
                _ => format_body_plan_examine_player(plan, carry),
            })
        }
    }
}

/// User-facing message when parent/prototype cannot be resolved.
pub fn format_no_parent_message(instance: &Object) -> String {
    if instance.object_type() == "player" {
        format!(
            "{} has no prototype object. Try: examine {}",
            instance.name,
            instance
                .creature_name()
                .unwrap_or_else(|| "human".to_string())
        )
    } else {
        format!("The {} has no parent prototype.", instance.name.to_lowercase())
    }
}

/// Keys copied from prototype at creation — used when listing inherited properties.
pub fn prototype_property_keys() -> &'static [&'static str] {
    PROTOTYPE_PROPERTY_KEYS
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display::DisplayMode;
    use crate::mudl::load_module;
    use crate::object::{PermissionFlags, Property, Value};

    fn test_anatomy() -> AnatomyRegistry {
        load_module("modules/default")
            .unwrap()
            .active_world()
            .unwrap()
            .anatomy
            .clone()
    }

    #[test]
    fn parse_examine_parent_forms() {
        assert_eq!(
            parse_examine_request(&["#parent"]),
            ExamineRequest::Target(ExamineTarget::PrototypeOf("self".into()))
        );
        assert_eq!(
            parse_examine_request(&["coins.parent"]),
            ExamineRequest::Target(ExamineTarget::PrototypeOf("coins".into()))
        );
        assert_eq!(
            parse_examine_request(&["coins", "parent"]),
            ExamineRequest::Target(ExamineTarget::PrototypeOf("coins".into()))
        );
        assert_eq!(
            parse_examine_request(&["human"]),
            ExamineRequest::Target(ExamineTarget::Object("human".into()))
        );
    }

    #[test]
    fn resolve_examine_human_as_body_plan() {
        let anatomy = test_anatomy();
        let observer = ObjectId::new("player:admin-001");
        let objects = HashMap::new();
        let request = parse_examine_request(&["human"]);
        let resolution = resolve_examine_request(
            &request,
            &anatomy,
            &observer,
            None,
            &objects,
        )
        .unwrap();
        assert_eq!(resolution, ExamineResolution::BodyPlan("human".into()));
    }

    #[test]
    fn resolve_prototype_of_item() {
        let observer = ObjectId::new("player:admin-001");
        let proto_id = ObjectId::new("item:coin-proto-001");
        let coin_id = ObjectId::new("item:coins-001");

        let proto = Object {
            id: proto_id.clone(),
            name: "Gold Coin".to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: observer.clone(),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        let mut coin = Object {
            id: coin_id.clone(),
            name: "coins".to_string(),
            aliases: Vec::new(),
            location: Some(observer.clone()),
            prototype: Some(proto_id.clone()),
            owner: observer.clone(),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        coin.set_property_int("weight", 1);

        let mut objects = HashMap::new();
        objects.insert(proto_id.clone(), proto);
        objects.insert(coin_id.clone(), coin);

        let request = ExamineRequest::Target(ExamineTarget::PrototypeOf("coins".into()));
        let resolution = resolve_examine_request(
            &request,
            &AnatomyRegistry::default(),
            &observer,
            None,
            &objects,
        )
        .unwrap();
        assert_eq!(
            resolution,
            ExamineResolution::Prototype {
                instance_id: coin_id,
                prototype_id: proto_id,
            }
        );
    }

    #[test]
    fn resolve_self_parent_falls_back_to_creature_body_plan() {
        let anatomy = test_anatomy();
        let observer = ObjectId::new("player:admin-001");
        let mut player = Object {
            id: observer.clone(),
            name: "Admin".to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: observer.clone(),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        player.add_property(Property {
            name: "creature".to_string(),
            value: Value::String("human".to_string()),
            permissions: PermissionFlags::OWNER,
            behavior: None,
        });

        let objects = HashMap::from([(observer.clone(), player)]);
        let request = parse_examine_request(&["#parent"]);
        let resolution = resolve_examine_request(
            &request,
            &anatomy,
            &observer,
            None,
            &objects,
        )
        .unwrap();
        assert_eq!(resolution, ExamineResolution::BodyPlan("human".into()));
    }

    #[test]
    fn format_prototype_examine_player_lists_inherited_props() {
        let observer = ObjectId::new("player:admin-001");
        let proto_id = ObjectId::new("item:coin-proto-001");
        let coin_id = ObjectId::new("item:coins-001");

        let mut proto = Object {
            id: proto_id.clone(),
            name: "Gold Coin".to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: observer.clone(),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        proto.set_property_int("weight", 1);
        proto.set_property_bool("stackable", true);
        proto.add_property(Property {
            name: "description".to_string(),
            value: Value::String("A shiny gold coin.".to_string()),
            permissions: PermissionFlags::EVERYONE,
            behavior: None,
        });

        let coin = Object {
            id: coin_id.clone(),
            name: "coins".to_string(),
            aliases: Vec::new(),
            location: Some(observer.clone()),
            prototype: Some(proto_id.clone()),
            owner: observer.clone(),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };

        let objects = HashMap::from([
            (proto_id, proto),
            (coin_id.clone(), coin.clone()),
        ]);
        let ctx = DisplayContext::new(observer, DisplayMode::Player).with_objects(objects);
        let output = format_examine_output(
            &ExamineResolution::Prototype {
                instance_id: coin_id,
                prototype_id: ObjectId::new("item:coin-proto-001"),
            },
            &ctx,
        )
        .unwrap();

        assert!(output.contains("Parent of coins"));
        assert!(output.contains("weight: 1"));
        assert!(output.contains("stackable: true"));
        assert!(output.contains("shiny gold coin"));
    }
}