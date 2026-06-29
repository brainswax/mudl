use std::collections::HashMap;

use crate::mudl::LoadedWorld;
use crate::object::{ObjectFactory, ObjectId, PermissionFlags, Property, Value};
use crate::persistence::Persistence;

/// Bootstrap world objects from a loaded MUDL world into persistence.
pub async fn bootstrap_world<P: Persistence>(
    factory: &ObjectFactory<P>,
    owner: ObjectId,
    world: &LoadedWorld,
) -> anyhow::Result<ObjectId> {
    if let Some(start_base) = &world.starting_location {
        let start_id = ObjectId::new(format!("room:{start_base}-001"));
        if factory.load_object(&start_id).await?.is_some() {
            return Ok(start_id);
        }
    }

    if world.world_defs.is_empty() {
        anyhow::bail!("No world definitions in world {}", world.name);
    }

    let mut name_to_id: HashMap<String, ObjectId> = HashMap::new();

    for def in &world.world_defs {
        let mut obj = factory
            .create(&def.obj_type, &def.base_name, owner.clone())
            .await?;
        obj.name = def.name.clone();
        if let Some(desc) = &def.description {
            obj.add_property(Property {
                name: "description".to_string(),
                value: Value::String(desc.clone()),
                permissions: PermissionFlags::EVERYONE,
                behavior: None,
            });
        }
        factory.persistence().save_object(&obj).await?;
        name_to_id.insert(def.base_name.clone(), obj.id.clone());
    }

    for def in &world.world_defs {
        if let Some(id) = name_to_id.get(&def.base_name) {
            let mut obj = if let Some(o) = factory.load_object(id).await? {
                o
            } else {
                continue;
            };
            if let Some(loc_base) = &def.location {
                if let Some(loc_id) = name_to_id.get(loc_base) {
                    obj.location = Some(loc_id.clone());
                }
            }
            for (dir, target_base) in &def.exits {
                if let Some(target_id) = name_to_id.get(target_base) {
                    obj.add_exit(dir, target_id.clone());
                }
            }
            factory.persistence().save_object(&obj).await?;
        }
    }

    if factory.load_object(&owner).await?.is_none() {
        let mut player = factory
            .create_player("admin", owner.clone(), &world.anatomy)
            .await?;
        player.name = "Admin".to_string();
        if let Some(start_base) = &world.starting_location {
            if let Some(start_id) = name_to_id.get(start_base) {
                player.location = Some(start_id.clone());
            }
        }
        factory.persistence().save_object(&player).await?;
    }

    let start_id = if let Some(start_base) = &world.starting_location {
        name_to_id
            .get(start_base)
            .cloned()
            .unwrap_or_else(|| ObjectId::new(format!("room:{}-001", start_base)))
    } else {
        name_to_id
            .values()
            .next()
            .cloned()
            .unwrap_or_else(|| ObjectId::new("room:the-void-001"))
    };

    Ok(start_id)
}