//! Object creation via the Factory pattern with role composition and prototype linking.

use std::collections::HashMap;

use crate::mudl::{AnatomyRegistry, PlayerTemplate};
use crate::object::{
    generate_object_id,
    roles::{ContainerSpec, StackableSpec, WearableSpec},
    slugify_display_name, Object, ObjectId, PermissionFlags,
};
use crate::persistence::Persistence;

pub struct ObjectFactory<P: Persistence> {
    persistence: P,
}

impl<P: Persistence> ObjectFactory<P> {
    pub fn new(persistence: P) -> Self {
        Self { persistence }
    }

    pub fn persistence(&self) -> &P {
        &self.persistence
    }

    pub async fn create_player(
        &self,
        display_name: &str,
        owner: ObjectId,
        anatomy: &AnatomyRegistry,
    ) -> anyhow::Result<Object> {
        let template = anatomy
            .default_template()
            .cloned()
            .unwrap_or(PlayerTemplate {
                name: "default".to_string(),
                creature: "human".to_string(),
                gender: "neutral".to_string(),
            });
        let slug = slugify_display_name(display_name);
        let mut player = self
            .create_named("player", &slug, display_name, owner)
            .await?;
        player.init_creature_role(&template);
        self.persistence.save_object(&player).await?;
        Ok(player)
    }

    pub async fn create_item(&self, display_name: &str, owner: ObjectId) -> anyhow::Result<Object> {
        let slug = slugify_display_name(display_name);
        let mut item = self
            .create_named("item", &slug, display_name, owner)
            .await?;
        item.init_item_defaults(true);
        self.persistence.save_object(&item).await?;
        Ok(item)
    }

    pub async fn create_container(
        &self,
        display_name: &str,
        owner: ObjectId,
        capacity: u32,
        wearable: bool,
    ) -> anyhow::Result<Object> {
        self.create_container_with_spec(
            display_name,
            owner,
            ContainerSpec {
                capacity,
                wearable,
                wear_slot: if wearable {
                    Some("torso".to_string())
                } else {
                    None
                },
                ..ContainerSpec::default()
            },
            None,
        )
        .await
    }

    /// Create a container with full capacity/weight/volume limits and optional prototype.
    pub async fn create_container_with_spec(
        &self,
        display_name: &str,
        owner: ObjectId,
        spec: ContainerSpec,
        prototype: Option<ObjectId>,
    ) -> anyhow::Result<Object> {
        let slug = slugify_display_name(display_name);
        let mut container = self
            .create_named("item", &slug, display_name, owner)
            .await?;
        if let Some(proto) = prototype {
            container.prototype = Some(proto);
        }
        container.apply_container_role(&spec);
        self.persistence.save_object(&container).await?;
        Ok(container)
    }

    /// Create a wearable item (garment, armor, etc.).
    pub async fn create_wearable(
        &self,
        display_name: &str,
        owner: ObjectId,
        spec: WearableSpec,
        prototype: Option<ObjectId>,
    ) -> anyhow::Result<Object> {
        let slug = slugify_display_name(display_name);
        let mut item = self
            .create_named("item", &slug, display_name, owner)
            .await?;
        if let Some(proto) = prototype {
            item.prototype = Some(proto);
        }
        item.apply_wearable_role(&spec);
        item.init_item_defaults(false);
        self.persistence.save_object(&item).await?;
        Ok(item)
    }

    /// Create a stackable item — one object instance representing `count` identical units.
    pub async fn create_stackable_item(
        &self,
        display_name: &str,
        owner: ObjectId,
        prototype: Option<ObjectId>,
        count: u32,
    ) -> anyhow::Result<Object> {
        let slug = slugify_display_name(display_name);
        let mut item = self
            .create_named("item", &slug, display_name, owner)
            .await?;
        if let Some(proto) = &prototype {
            item.prototype = Some(proto.clone());
            if let Some(proto_obj) = self.persistence.load_object(proto).await? {
                self.apply_prototype_defaults(&mut item, &proto_obj);
            }
        }
        item.init_item_defaults(true);
        item.apply_stackable_role(&StackableSpec {
            count: count.max(1),
            max_stack: 99,
        });
        self.persistence.save_object(&item).await?;
        Ok(item)
    }

    /// Spawn `count` separate object instances from a prototype (non-stacked).
    pub async fn create_item_instances(
        &self,
        display_name: &str,
        owner: ObjectId,
        prototype: Option<ObjectId>,
        count: u32,
    ) -> anyhow::Result<Vec<Object>> {
        let mut items = Vec::with_capacity(count as usize);
        for _ in 0..count.max(1) {
            let slug = slugify_display_name(display_name);
            let mut item = self
                .create_named("item", &slug, display_name, owner.clone())
                .await?;
            if let Some(proto) = &prototype {
                item.prototype = Some(proto.clone());
                if let Some(proto_obj) = self.persistence.load_object(proto).await? {
                    self.apply_prototype_defaults(&mut item, &proto_obj);
                }
            } else {
                item.init_item_defaults(true);
            }
            self.persistence.save_object(&item).await?;
            items.push(item);
        }
        Ok(items)
    }

    /// Copy role-relevant properties from a prototype onto a new object.
    pub fn apply_prototype_defaults(&self, target: &mut Object, prototype: &Object) {
        target.prototype = Some(prototype.id.clone());
        for key in [
            "weight",
            "volume",
            "is_container",
            "is_wearable",
            "is_pocketable",
            "capacity",
            "max_weight",
            "max_volume",
            "wear_slot",
            "hand_slot",
            "stackable",
            "max_stack",
            "description",
        ] {
            if let Some(prop) = prototype.get_property(key) {
                target.add_property(prop.clone());
            }
        }
    }

    /// Create an object using a slug for the ID and a separate display name.
    pub async fn create_named(
        &self,
        type_name: &str,
        slug: &str,
        display_name: &str,
        owner: ObjectId,
    ) -> anyhow::Result<Object> {
        let slug = slugify_display_name(slug);
        let type_name = type_name.to_ascii_lowercase();
        let counter = self
            .persistence
            .get_next_id_counter(&type_name, &slug)
            .await?;
        let id = generate_object_id(&type_name, &slug, counter);
        self.persistence
            .increment_counter(&type_name, &slug)
            .await?;

        let object = Object {
            id,
            name: display_name.to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner,
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };

        self.persistence.save_object(&object).await?;
        Ok(object)
    }

    /// Create an object where the slug and display name are the same (bootstrap/tests).
    pub async fn create(
        &self,
        type_name: &str,
        slug: &str,
        owner: ObjectId,
    ) -> anyhow::Result<Object> {
        self.create_named(type_name, slug, slug, owner).await
    }

    pub async fn load_object(&self, id: &ObjectId) -> anyhow::Result<Option<Object>> {
        self.persistence.load_object(id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::SqlitePersistence;

    async fn memory_factory() -> ObjectFactory<SqlitePersistence> {
        ObjectFactory::new(SqlitePersistence::new(":memory:").await.unwrap())
    }

    #[tokio::test]
    async fn create_stackable_item_has_count_and_prototype() {
        let factory = memory_factory().await;
        let owner = ObjectId::new("player:hero-001");

        let proto = factory
            .create_item("Gold Coin", owner.clone())
            .await
            .unwrap();
        let stack = factory
            .create_stackable_item("Gold Coin", owner, Some(proto.id.clone()), 25)
            .await
            .unwrap();

        assert_eq!(stack.stack_count(), 25);
        assert!(stack.is_stackable());
        assert_eq!(stack.prototype.as_ref(), Some(&proto.id));
    }

    #[tokio::test]
    async fn create_item_instances_produces_distinct_ids() {
        let factory = memory_factory().await;
        let owner = ObjectId::new("player:hero-001");

        let items = factory
            .create_item_instances("Arrow", owner, None, 3)
            .await
            .unwrap();

        assert_eq!(items.len(), 3);
        let ids: std::collections::HashSet<_> = items.iter().map(|o| o.id.as_str()).collect();
        assert_eq!(ids.len(), 3);
    }

    #[tokio::test]
    async fn create_container_with_weight_limit() {
        let factory = memory_factory().await;
        let owner = ObjectId::new("player:hero-001");

        let bag = factory
            .create_container_with_spec(
                "Strongbox",
                owner,
                ContainerSpec {
                    capacity: 20,
                    max_weight: Some(50),
                    max_volume: Some(30),
                    wearable: false,
                    wear_slot: None,
                },
                None,
            )
            .await
            .unwrap();

        assert_eq!(bag.container_max_weight(), Some(50));
        assert_eq!(bag.container_max_volume(), Some(30));
    }
}
