//! Object creation via the Factory pattern with role composition and prototype linking.
//!
//! Creation follows a fixed pipeline so IDs, prototypes, roles, and defaults never
//! clobber one another:
//!
//! 1. **Allocate** — reserve ID counter, persist empty shell
//! 2. **Prototype** — optional link + inherit role-relevant properties from DB
//! 3. **Role** — apply container / wearable / stackable / creature / plain item
//! 4. **Defaults** — fill unset generic item fields (`init_item_defaults_if_unset`)
//! 5. **Commit** — persist final object

use std::collections::HashMap;

use crate::creature::bootstrap_creature_behavior_system;
use crate::creature::init_creature_vitality;
use crate::mudl::{AnatomyRegistry, BehaviorTemplateDef, MudlRoleProps, NpcDef, PlayerTemplate};
use crate::object::{
    constrain_id_base, generate_object_id, id_base_from_display_name,
    roles::{ContainerSpec, KeySpec, StackableSpec, WearableSpec},
    slugify_display_name, Object, ObjectId, PermissionFlags, Property, Value,
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

    // --- Public creators ---

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
        let slug = id_base_from_display_name(display_name);
        let mut player = self
            .allocate_named("player", &slug, display_name, owner)
            .await?;
        player.init_creature_role(&template);
        if let Some(def) = anatomy.creature(&template.creature) {
            init_creature_vitality(&mut player, def);
        }
        self.commit(&mut player).await?;
        Ok(player)
    }

    /// Create an NPC from a MUDL `@npc` definition (idempotent via fixed `npc:<base>-001` id).
    pub async fn create_npc(
        &self,
        def: &NpcDef,
        owner: ObjectId,
        anatomy: &AnatomyRegistry,
        location: Option<ObjectId>,
        behavior_templates: &HashMap<String, BehaviorTemplateDef>,
    ) -> anyhow::Result<Object> {
        let npc_id = ObjectId::new(format!("npc:{}-001", def.base_name));
        if let Some(existing) = self.persistence.load_object(&npc_id).await? {
            return Ok(existing);
        }
        let display_name = def.name.as_deref().unwrap_or(&def.base_name);
        let mut npc = Object {
            id: npc_id,
            name: display_name.to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner,
            permissions: PermissionFlags::EVERYONE,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
            revision: 0,
            updated_at: None,
        };
        let meta = self.persistence.save_object(&npc).await?;
        meta.apply_to(&mut npc);
        let template = PlayerTemplate {
            name: def.base_name.clone(),
            creature: def.creature.clone(),
            gender: "neutral".to_string(),
        };
        npc.init_creature_role(&template);
        if let Some(creature_def) = anatomy.creature(&def.creature) {
            init_creature_vitality(&mut npc, creature_def);
        }
        if let Some(loc) = location {
            npc.location = Some(loc);
        }
        bootstrap_creature_behavior_system(
            &mut npc,
            &def.behaviors,
            &def.use_behaviors,
            behavior_templates,
            &def.triggers,
        );
        self.commit(&mut npc).await?;
        Ok(npc)
    }

    pub async fn create_item(&self, display_name: &str, owner: ObjectId) -> anyhow::Result<Object> {
        let slug = id_base_from_display_name(display_name);
        let mut item = self
            .allocate_named("item", &slug, display_name, owner)
            .await?;
        Self::fill_item_defaults(&mut item, true);
        self.commit(&mut item).await?;
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
                ..crate::object::ContainerSpec::default()
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
        let slug = id_base_from_display_name(display_name);
        let mut container = self
            .allocate_named("item", &slug, display_name, owner)
            .await?;
        self.attach_prototype(&mut container, prototype).await?;
        container.apply_container_role(&spec);
        self.commit(&mut container).await?;
        Ok(container)
    }

    /// Create a key that opens any lock sharing `lock_id`.
    pub async fn create_key(
        &self,
        display_name: &str,
        owner: ObjectId,
        lock_id: &str,
        prototype: Option<ObjectId>,
    ) -> anyhow::Result<Object> {
        let slug = id_base_from_display_name(display_name);
        let mut key = self
            .allocate_named("item", &slug, display_name, owner)
            .await?;
        self.attach_prototype(&mut key, prototype).await?;
        key.apply_key_role(&KeySpec::new(lock_id));
        Self::fill_item_defaults(&mut key, true);
        self.commit(&mut key).await?;
        Ok(key)
    }

    /// Create a wearable item (garment, armor, etc.).
    pub async fn create_wearable(
        &self,
        display_name: &str,
        owner: ObjectId,
        spec: WearableSpec,
        prototype: Option<ObjectId>,
    ) -> anyhow::Result<Object> {
        let slug = id_base_from_display_name(display_name);
        let mut item = self
            .allocate_named("item", &slug, display_name, owner)
            .await?;
        self.attach_prototype(&mut item, prototype).await?;
        item.apply_wearable_role(&spec);
        Self::fill_item_defaults(&mut item, false);
        self.commit(&mut item).await?;
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
        let slug = id_base_from_display_name(display_name);
        let mut item = self
            .allocate_named("item", &slug, display_name, owner)
            .await?;
        self.attach_prototype(&mut item, prototype).await?;
        Self::fill_item_defaults(&mut item, true);
        item.apply_stackable_role(&StackableSpec {
            count: count.max(1),
            max_stack: 99,
        });
        self.commit(&mut item).await?;
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
            let slug = id_base_from_display_name(display_name);
            let mut item = self
                .allocate_named("item", &slug, display_name, owner.clone())
                .await?;
            self.attach_prototype(&mut item, prototype.clone()).await?;
            Self::fill_item_defaults(&mut item, true);
            self.commit(&mut item).await?;
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
            "is_open",
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
            "is_readable",
            "read_text",
            "is_writable",
            "write_text",
            "lock_id",
            "is_locked",
            "lock_consumable",
            "is_key",
            "key_consumable",
            "is_portal",
            "is_door",
            "is_window",
            "portal_kind",
            "portal_passable",
            "portal_transparent",
            "mod_max_weight",
            "mod_encumbrance",
            "mod_max_health",
            "mod_stats",
            "mod_skills",
            "grant_effects",
            "door_direction",
            "door_destination_base",
            "allowed_types",
            "is_breakable",
            "break_text",
            "harvestable",
            "hidden_until_discovered",
            "discovery_stealth",
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
        let object = self
            .allocate_named(type_name, slug, display_name, owner)
            .await?;
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

    /// Create a navigable place (`area` or `room`) for runtime world building.
    pub async fn create_place(
        &self,
        place_type: &str,
        display_name: &str,
        owner: ObjectId,
        description: Option<&str>,
        parent: Option<ObjectId>,
    ) -> anyhow::Result<Object> {
        let place_type = place_type.to_ascii_lowercase();
        if place_type != "area" && place_type != "room" {
            anyhow::bail!("place type must be area or room, got {place_type}");
        }
        let slug = id_base_from_display_name(display_name);
        let mut place = self
            .create_named(&place_type, &slug, display_name, owner)
            .await?;
        if let Some(desc) = description {
            place.add_property(Property {
                name: "description".to_string(),
                value: Value::String(desc.to_string()),
                permissions: PermissionFlags::EVERYONE,
                behavior: None,
            });
        }
        if let Some(parent_id) = parent {
            place.location = Some(parent_id);
        }
        self.commit(&mut place).await?;
        Ok(place)
    }

    pub async fn load_object(&self, id: &ObjectId) -> anyhow::Result<Option<Object>> {
        self.persistence.load_object(id).await
    }

    /// Materialize an item from MUDL prototype/instance definitions.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_from_mudl_spec(
        &self,
        base_name: &str,
        display_name: &str,
        owner: ObjectId,
        prototype_id: Option<ObjectId>,
        props: &MudlRoleProps,
        description: Option<&str>,
        aliases: &[String],
    ) -> anyhow::Result<Object> {
        let mut obj = self
            .allocate_named("item", base_name, display_name, owner)
            .await?;
        self.attach_prototype(&mut obj, prototype_id).await?;
        props.apply_to(&mut obj);
        let pocketable = props
            .pocketable
            .unwrap_or(!(props.is_container.unwrap_or(false)));
        Self::fill_item_defaults(&mut obj, pocketable);
        if let Some(desc) = description {
            obj.add_property(Property {
                name: "description".to_string(),
                value: Value::String(desc.to_string()),
                permissions: PermissionFlags::EVERYONE,
                behavior: None,
            });
        }
        if !aliases.is_empty() {
            obj.aliases = aliases.to_vec();
        }
        self.commit(&mut obj).await?;
        Ok(obj)
    }

    // --- Pipeline stages ---

    /// Stage 1: reserve ID counter and persist an empty shell.
    async fn allocate_named(
        &self,
        type_name: &str,
        slug: &str,
        display_name: &str,
        owner: ObjectId,
    ) -> anyhow::Result<Object> {
        let slug = constrain_id_base(&slugify_display_name(slug));
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
            revision: 0,
            updated_at: None,
        };

        let meta = self.persistence.save_object(&object).await?;
        let mut object = object;
        meta.apply_to(&mut object);
        Ok(object)
    }

    /// Stage 2: link prototype and inherit stored properties when present.
    async fn attach_prototype(
        &self,
        object: &mut Object,
        prototype: Option<ObjectId>,
    ) -> anyhow::Result<()> {
        let Some(proto_id) = prototype else {
            return Ok(());
        };
        object.prototype = Some(proto_id.clone());
        if let Some(proto_obj) = self.persistence.load_object(&proto_id).await? {
            self.apply_prototype_defaults(object, &proto_obj);
        }
        Ok(())
    }

    /// Stage 4: generic item defaults that do not override prototype or role values.
    fn fill_item_defaults(object: &mut Object, pocketable: bool) {
        object.init_item_defaults_if_unset(pocketable);
    }

    /// Stage 5: persist the finalized object.
    async fn commit(&self, object: &mut Object) -> anyhow::Result<()> {
        let meta = self.persistence.save_object(object).await?;
        meta.apply_to(object);
        Ok(())
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
    async fn create_stackable_inherits_prototype_weight_without_overwrite() {
        let factory = memory_factory().await;
        let owner = ObjectId::new("player:hero-001");

        let mut proto = factory
            .create_item("Gold Coin", owner.clone())
            .await
            .unwrap();
        proto.set_property_numeric("weight", 0.25);
        factory.persistence().save_object(&proto).await.unwrap();

        let stack = factory
            .create_stackable_item("Gold Coin", owner, Some(proto.id.clone()), 4)
            .await
            .unwrap();

        assert!((stack.unit_weight() - 0.25).abs() < f64::EPSILON);
        assert!((stack.weight() - 1.0).abs() < f64::EPSILON);
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
    async fn create_player_has_default_max_weight() {
        let factory = memory_factory().await;
        let anatomy = crate::mudl::load_module("modules/default")
            .unwrap()
            .active_world()
            .unwrap()
            .anatomy
            .clone();
        let owner = ObjectId::new("player:hero-001");

        let player = factory
            .create_player("hero", owner, &anatomy)
            .await
            .unwrap();

        assert_eq!(
            player.get_int_property("max_weight"),
            Some(crate::object::DEFAULT_PLAYER_MAX_WEIGHT)
        );
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
                    ..crate::object::ContainerSpec::default()
                },
                None,
            )
            .await
            .unwrap();

        assert_eq!(bag.container_max_weight(), Some(50));
        assert_eq!(bag.container_max_volume(), Some(30));
    }

    #[tokio::test]
    async fn create_wearable_applies_wear_slot_and_phys() {
        let factory = memory_factory().await;
        let owner = ObjectId::new("player:hero-001");

        let cloak = factory
            .create_wearable("Cloak", owner, WearableSpec::new("back", 2.5, 3.0), None)
            .await
            .unwrap();

        assert!(cloak.is_wearable());
        assert_eq!(cloak.wear_slot().as_deref(), Some("back"));
        assert!((cloak.weight() - 2.5).abs() < f64::EPSILON);
        assert!((cloak.volume() - 3.0).abs() < f64::EPSILON);
        assert!(!cloak.get_bool_property("is_pocketable").unwrap_or(true));
    }

    #[tokio::test]
    async fn create_wearable_role_wins_over_prototype_phys() {
        let factory = memory_factory().await;
        let owner = ObjectId::new("player:hero-001");

        let mut proto = factory
            .create_item("Robe Template", owner.clone())
            .await
            .unwrap();
        proto.set_property_numeric("weight", 5.0);
        factory.persistence().save_object(&proto).await.unwrap();

        let robe = factory
            .create_wearable(
                "Silk Robe",
                owner,
                WearableSpec::new("torso", 1.2, 2.0),
                Some(proto.id.clone()),
            )
            .await
            .unwrap();

        assert!((robe.weight() - 1.2).abs() < f64::EPSILON);
        assert!((robe.volume() - 2.0).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn create_container_sets_capacity_and_contents() {
        let factory = memory_factory().await;
        let owner = ObjectId::new("player:hero-001");

        let crate_obj = factory
            .create_container("crate", owner, 12, false)
            .await
            .unwrap();

        assert!(crate_obj.is_container());
        assert_eq!(crate_obj.container_capacity(), 12);
        assert!(!crate_obj.is_wearable());
        assert!(crate_obj.container_contents().is_empty());
    }

    #[tokio::test]
    async fn create_player_persists_creature_role() {
        let factory = memory_factory().await;
        let anatomy = crate::mudl::load_module("modules/default")
            .unwrap()
            .active_world()
            .unwrap()
            .anatomy
            .clone();
        let owner = ObjectId::new("player:hero-001");

        let player = factory
            .create_player("hero", owner.clone(), &anatomy)
            .await
            .unwrap();

        let reloaded = factory.load_object(&player.id).await.unwrap().unwrap();
        assert!(reloaded.has_creature_role());
        assert_eq!(reloaded.body_plan_name(), Some("human".to_string()));
        assert!(reloaded.body_slots().is_empty());
    }
}
