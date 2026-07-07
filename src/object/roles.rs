//! Composable object roles stored as properties (composition over inheritance).

use std::collections::HashMap;

use crate::mudl::PlayerTemplate;
use crate::object::{Object, ObjectId, PermissionFlags, Property, Value};

/// Role kinds attachable to any object via properties.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RoleKind {
    Location,
    Container,
    Wearable,
    Creature,
    Stackable,
}

/// Summary of which roles an object currently has.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ObjectRoles {
    pub location: bool,
    pub container: bool,
    pub wearable: bool,
    pub creature: bool,
    pub stackable: bool,
}

/// Configuration for a container role.
#[derive(Debug, Clone)]
pub struct ContainerSpec {
    pub capacity: u32,
    pub max_weight: Option<i64>,
    pub max_volume: Option<i64>,
    pub wearable: bool,
    pub wear_slot: Option<String>,
    /// When false, contents are hidden and inaccessible until opened.
    pub open: bool,
    /// Optional lock identifier — keys with a matching `lock_id` can unlock this container.
    pub lock_id: Option<String>,
    /// Whether the container starts locked (requires `lock_id`).
    pub locked: bool,
    /// When true, the lock mechanism is spent after a successful unlock and cannot be used again.
    pub lock_consumable: bool,
    /// When set, only items matching at least one type tag may be placed inside (e.g. `key`).
    pub allowed_types: Option<Vec<String>>,
}

impl Default for ContainerSpec {
    fn default() -> Self {
        Self {
            capacity: 10,
            max_weight: None,
            max_volume: None,
            wearable: false,
            wear_slot: None,
            open: true,
            lock_id: None,
            locked: false,
            lock_consumable: false,
            allowed_types: None,
        }
    }
}

/// Player-facing label for a type tag in restriction messages.
pub fn allowed_type_label(type_tag: &str) -> String {
    match type_tag.trim().to_ascii_lowercase().as_str() {
        "key" => "keys".to_string(),
        "readable" => "readable items".to_string(),
        "stackable" => "stackable items".to_string(),
        "wearable" => "wearable items".to_string(),
        "container" => "containers".to_string(),
        other => other.to_string(),
    }
}

/// Join allowed-type labels for messages (`keys`, `keys and tokens`).
pub fn format_allowed_type_labels(types: &[String]) -> String {
    let labels: Vec<String> = types.iter().map(|t| allowed_type_label(t)).collect();
    match labels.len() {
        0 => String::new(),
        1 => labels.into_iter().next().unwrap_or_default(),
        2 => format!("{} and {}", labels[0], labels[1]),
        _ => {
            let mut rest = labels;
            let last = rest.pop().unwrap();
            format!("{}, and {}", rest.join(", "), last)
        }
    }
}

/// Parse a comma-separated allowed-types field (`key`, `key,token`).
pub fn parse_allowed_types(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|t| t.trim().to_ascii_lowercase())
        .filter(|t| !t.is_empty())
        .collect()
}

/// Configuration for a key that opens one or more locks sharing the same `lock_id`.
#[derive(Debug, Clone)]
pub struct KeySpec {
    pub lock_id: String,
    /// When true, the key is destroyed after a successful unlock.
    pub consumable: bool,
}

impl KeySpec {
    pub fn new(lock_id: impl Into<String>) -> Self {
        Self {
            lock_id: lock_id.into(),
            consumable: false,
        }
    }

    pub fn consumable(mut self) -> Self {
        self.consumable = true;
        self
    }
}

/// Configuration for a breakable prop — smashing it can disable spawners and drop loot.
#[derive(Debug, Clone, Default)]
pub struct BreakableSpec {
    /// Optional custom narrative when the object is broken.
    pub break_text: Option<String>,
}

/// Kind of exit portal — doors, windows, and future teleporters share one model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PortalKind {
    Door,
    Window,
    Teleport,
}

impl PortalKind {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "door" => Some(Self::Door),
            "window" => Some(Self::Window),
            "teleport" | "portal" => Some(Self::Teleport),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Door => "door",
            Self::Window => "window",
            Self::Teleport => "teleport",
        }
    }

    pub fn default_passable(self) -> bool {
        matches!(self, Self::Door | Self::Teleport)
    }

    pub fn default_transparent(self) -> bool {
        matches!(self, Self::Window)
    }

    /// Player-facing noun for messages (`door`, `window`, `portal`).
    pub fn label(self) -> &'static str {
        match self {
            Self::Door => "door",
            Self::Window => "window",
            Self::Teleport => "portal",
        }
    }
}

/// Configuration for an exit portal (door, window, or teleport).
#[derive(Debug, Clone)]
pub struct PortalSpec {
    pub kind: PortalKind,
    /// Exit or view direction (`in`, `out`, `north`, …).
    pub direction: String,
    /// Destination area base name (resolved to an object id at bootstrap).
    pub destination: String,
    /// Whether the portal starts open. Doors and windows default to closed.
    pub open: bool,
    pub lock_id: Option<String>,
    pub locked: bool,
    /// When true, the lock mechanism is spent after a successful unlock.
    pub lock_consumable: bool,
    /// When set, overrides the kind default (windows are not passable by default).
    pub passable: Option<bool>,
    /// When set, overrides the kind default (windows are transparent by default).
    pub transparent: Option<bool>,
}

impl PortalSpec {
    pub fn new(kind: PortalKind, direction: impl Into<String>, destination: impl Into<String>) -> Self {
        Self {
            kind,
            direction: direction.into(),
            destination: destination.into(),
            open: false,
            lock_id: None,
            locked: false,
            lock_consumable: false,
            passable: None,
            transparent: None,
        }
    }
}

/// Configuration for a door — convenience wrapper around [`PortalSpec`].
#[derive(Debug, Clone)]
pub struct DoorSpec {
    /// Exit direction this door guards (`in`, `out`, `north`, …).
    pub direction: String,
    /// Destination area base name (resolved to an object id at bootstrap).
    pub destination: String,
    /// Whether the door starts open. Doors default to closed.
    pub open: bool,
    pub lock_id: Option<String>,
    pub locked: bool,
    /// When true, the lock mechanism is spent after a successful unlock.
    pub lock_consumable: bool,
}

impl DoorSpec {
    pub fn new(direction: impl Into<String>, destination: impl Into<String>) -> Self {
        Self {
            direction: direction.into(),
            destination: destination.into(),
            open: false,
            lock_id: None,
            locked: false,
            lock_consumable: false,
        }
    }
}

/// Configuration for a wearable role.
#[derive(Debug, Clone)]
pub struct WearableSpec {
    pub wear_slot: String,
    pub weight: f64,
    pub volume: f64,
    /// Additive bonus to the wearer's effective `max_weight` while equipped.
    pub mod_max_weight: Option<i64>,
    /// Encumbrance multiplier while equipped (`0.85` = 15% lighter feel for movement).
    pub mod_encumbrance: Option<f64>,
    /// Additive bonus to effective max health while equipped.
    pub mod_max_health: Option<i64>,
    pub stat_mods: HashMap<String, i64>,
    pub skill_mods: HashMap<String, i64>,
    pub grant_effects: Vec<String>,
}

impl WearableSpec {
    pub fn new(wear_slot: impl Into<String>, weight: f64, volume: f64) -> Self {
        Self {
            wear_slot: wear_slot.into(),
            weight,
            volume,
            mod_max_weight: None,
            mod_encumbrance: None,
            mod_max_health: None,
            stat_mods: HashMap::new(),
            skill_mods: HashMap::new(),
            grant_effects: Vec::new(),
        }
    }
}

/// Physical attributes for a generic item.
#[derive(Debug, Clone)]
pub struct ItemPhysSpec {
    pub weight: f64,
    pub volume: f64,
    pub pocketable: bool,
}

impl Default for ItemPhysSpec {
    fn default() -> Self {
        Self {
            weight: 1.0,
            volume: 1.0,
            pocketable: true,
        }
    }
}

/// Configuration for stackable identical items.
#[derive(Debug, Clone)]
pub struct StackableSpec {
    pub count: u32,
    pub max_stack: u32,
}

/// Configuration for objects with readable (and optionally writable) text.
#[derive(Debug, Clone)]
pub struct ReadableSpec {
    pub text: String,
    pub writable: bool,
}

impl Default for StackableSpec {
    fn default() -> Self {
        Self {
            count: 1,
            max_stack: 99,
        }
    }
}

impl Object {
    /// Inspect which composable roles are active on this object.
    pub fn roles(&self) -> ObjectRoles {
        ObjectRoles {
            location: self.is_location(),
            container: self.has_container_role(),
            wearable: self.has_wearable_role(),
            creature: self.has_creature_role(),
            stackable: self.is_stackable(),
        }
    }

    pub fn has_container_role(&self) -> bool {
        self.get_bool_property("is_container").unwrap_or(false)
    }

    pub fn has_wearable_role(&self) -> bool {
        self.get_bool_property("is_wearable").unwrap_or(false)
    }

    pub fn has_creature_role(&self) -> bool {
        self.object_type() == "player" || self.get_property("creature").is_some()
    }

    pub fn is_stackable(&self) -> bool {
        self.get_bool_property("stackable").unwrap_or(false)
    }

    pub fn weight(&self) -> f64 {
        let unit = self.unit_weight();
        if self.is_stackable() {
            unit * f64::from(self.stack_count())
        } else {
            unit
        }
    }

    pub fn unit_weight(&self) -> f64 {
        self.get_numeric_property("weight").unwrap_or(1.0)
    }

    pub fn volume(&self) -> f64 {
        let unit = self.unit_volume();
        if self.is_stackable() {
            unit * f64::from(self.stack_count())
        } else {
            unit
        }
    }

    pub fn unit_volume(&self) -> f64 {
        self.get_numeric_property("volume").unwrap_or(1.0)
    }

    pub fn stack_count(&self) -> u32 {
        self.get_int_property("stack_count").unwrap_or(1) as u32
    }

    pub fn max_stack(&self) -> u32 {
        self.get_int_property("max_stack").unwrap_or(99) as u32
    }

    pub fn set_stack_count(&mut self, count: u32) {
        self.set_property_int("stack_count", i64::from(count));
    }

    pub fn container_capacity(&self) -> u32 {
        self.get_int_property("capacity").unwrap_or(10) as u32
    }

    pub fn container_max_weight(&self) -> Option<i64> {
        self.get_int_property("max_weight")
    }

    pub fn container_max_volume(&self) -> Option<i64> {
        self.get_int_property("max_volume")
    }

    pub fn container_contents(&self) -> Vec<ObjectId> {
        self.get_object_list_property("contents")
    }

    /// Type tags this container accepts. `None` means no restriction.
    pub fn container_allowed_types(&self) -> Option<Vec<String>> {
        self.get_string_property("allowed_types")
            .map(|s| parse_allowed_types(&s))
            .filter(|types| !types.is_empty())
    }

    /// Whether `item` satisfies a composable type tag (`key`, `stackable`, `readable`, …).
    pub fn item_has_type(&self, type_tag: &str) -> bool {
        match type_tag.trim().to_ascii_lowercase().as_str() {
            "key" => self.is_key(),
            "stackable" => self.is_stackable(),
            "readable" => self.is_readable(),
            "wearable" => self.is_wearable(),
            "container" => self.is_container(),
            other => self.object_type().eq_ignore_ascii_case(other),
        }
    }

    /// Whether `item` may be placed in this container (allowed-types filter).
    pub fn container_accepts_item(&self, item: &Object) -> bool {
        match self.container_allowed_types() {
            None => true,
            Some(allowed) => allowed.iter().any(|tag| item.item_has_type(tag)),
        }
    }

    /// Whether a container's lid is open. Missing property defaults to open (legacy objects).
    pub fn container_is_open(&self) -> bool {
        if !self.has_container_role() {
            return true;
        }
        self.get_bool_property("is_open").unwrap_or(true)
    }

    pub fn set_container_open(&mut self, open: bool) {
        self.set_property_bool("is_open", open);
    }

    /// Whether this container has a lock mechanism (`lock_id` set).
    pub fn container_has_lock(&self) -> bool {
        self.container_lock_id().is_some()
    }

    pub fn container_lock_id(&self) -> Option<String> {
        self.get_string_property("lock_id")
            .filter(|id| !id.trim().is_empty())
    }

    pub fn set_container_lock_id(&mut self, lock_id: impl Into<String>) {
        self.set_property_string("lock_id", lock_id);
    }

    /// Whether the container is currently locked. Ignored when no `lock_id` is set.
    pub fn container_is_locked(&self) -> bool {
        self.container_has_lock() && self.get_bool_property("is_locked").unwrap_or(false)
    }

    pub fn set_container_locked(&mut self, locked: bool) {
        self.set_property_bool("is_locked", locked);
    }

    /// Ensure a container has a lock id, generating one from its object id if needed.
    pub fn ensure_container_lock_id(&mut self) -> String {
        if let Some(id) = self.container_lock_id() {
            return id;
        }
        let lock_id = format!("lock:{}", self.id.as_str());
        self.set_container_lock_id(&lock_id);
        lock_id
    }

    pub fn is_key(&self) -> bool {
        self.get_bool_property("is_key").unwrap_or(false)
    }

    pub fn key_lock_id(&self) -> Option<String> {
        self.get_string_property("lock_id")
            .filter(|id| !id.trim().is_empty())
    }

    pub fn key_consumable(&self) -> bool {
        self.get_bool_property("key_consumable").unwrap_or(false)
    }

    pub fn lock_consumable(&self) -> bool {
        self.get_bool_property("lock_consumable").unwrap_or(false)
    }

    pub fn apply_key_role(&mut self, spec: &KeySpec) {
        self.set_property_bool("is_key", true);
        self.set_property_string("lock_id", &spec.lock_id);
        if spec.consumable {
            self.set_property_bool("key_consumable", true);
        }
        self.set_property_bool("is_pocketable", true);
        if self.get_numeric_property("weight").is_none() {
            self.set_property_numeric("weight", 0.1);
        }
        if self.get_numeric_property("volume").is_none() {
            self.set_property_numeric("volume", 0.1);
        }
    }

    pub fn is_breakable(&self) -> bool {
        self.get_bool_property("is_breakable").unwrap_or(false)
    }

    pub fn break_text(&self) -> Option<String> {
        self.get_string_property("break_text")
            .filter(|text| !text.trim().is_empty())
    }

    pub fn apply_breakable_role(&mut self, spec: &BreakableSpec) {
        self.set_property_bool("is_breakable", true);
        if let Some(text) = &spec.break_text {
            self.set_property_string("break_text", text);
        }
    }

    pub fn portal_kind(&self) -> Option<PortalKind> {
        self.get_string_property("portal_kind")
            .and_then(|s| PortalKind::parse(&s))
            .or_else(|| {
                if self.get_bool_property("is_window").unwrap_or(false) {
                    Some(PortalKind::Window)
                } else if self.get_bool_property("is_door").unwrap_or(false) {
                    Some(PortalKind::Door)
                } else {
                    None
                }
            })
    }

    pub fn is_portal(&self) -> bool {
        self.portal_kind().is_some()
    }

    pub fn is_door(&self) -> bool {
        self.portal_kind() == Some(PortalKind::Door)
    }

    pub fn is_window(&self) -> bool {
        self.portal_kind() == Some(PortalKind::Window)
    }

    pub fn portal_direction(&self) -> Option<String> {
        self.get_string_property("door_direction")
    }

    pub fn door_direction(&self) -> Option<String> {
        self.portal_direction()
    }

    /// Resolved destination room id (set at bootstrap).
    pub fn portal_destination(&self) -> Option<ObjectId> {
        self.get_object_ref_property("door_destination")
    }

    pub fn door_destination(&self) -> Option<ObjectId> {
        self.portal_destination()
    }

    /// Destination area base name from MUDL (before bootstrap resolution).
    pub fn portal_destination_base(&self) -> Option<String> {
        self.get_string_property("door_destination_base")
    }

    pub fn door_destination_base(&self) -> Option<String> {
        self.portal_destination_base()
    }

    pub fn set_portal_destination(&mut self, destination: ObjectId) {
        self.set_property_object_ref("door_destination", destination);
    }

    pub fn set_door_destination(&mut self, destination: ObjectId) {
        self.set_portal_destination(destination);
    }

    pub fn portal_passable(&self) -> bool {
        self.get_bool_property("portal_passable")
            .unwrap_or_else(|| {
                self.portal_kind()
                    .map(PortalKind::default_passable)
                    .unwrap_or(false)
            })
    }

    pub fn portal_transparent(&self) -> bool {
        self.get_bool_property("portal_transparent")
            .unwrap_or_else(|| {
                self.portal_kind()
                    .map(PortalKind::default_transparent)
                    .unwrap_or(false)
            })
    }

    /// Whether the player can see through this portal into its destination.
    pub fn portal_allows_view(&self) -> bool {
        if !self.is_portal() {
            return false;
        }
        if self.gate_is_locked() {
            return false;
        }
        self.portal_transparent() || self.gate_is_open()
    }

    pub fn apply_portal_role(&mut self, spec: &PortalSpec) {
        self.set_property_bool("is_portal", true);
        self.set_property_string("portal_kind", spec.kind.as_str());
        match spec.kind {
            PortalKind::Door => self.set_property_bool("is_door", true),
            PortalKind::Window => self.set_property_bool("is_window", true),
            PortalKind::Teleport => {}
        }
        self.set_property_string("door_direction", &spec.direction);
        self.set_property_string("door_destination_base", &spec.destination);
        self.set_property_bool("is_open", spec.open);
        self.set_property_bool(
            "portal_passable",
            spec.passable.unwrap_or_else(|| spec.kind.default_passable()),
        );
        self.set_property_bool(
            "portal_transparent",
            spec.transparent
                .unwrap_or_else(|| spec.kind.default_transparent()),
        );
        self.set_property_bool("is_pocketable", false);
        if let Some(ref lock_id) = spec.lock_id {
            self.set_container_lock_id(lock_id);
            self.set_container_locked(spec.locked);
            if spec.lock_consumable {
                self.set_property_bool("lock_consumable", true);
            }
        }
        let (default_weight, default_volume) = match spec.kind {
            PortalKind::Window => (2.0, 1.0),
            PortalKind::Door => (5.0, 4.0),
            PortalKind::Teleport => (1.0, 1.0),
        };
        if self.get_numeric_property("weight").is_none() {
            self.set_property_numeric("weight", default_weight);
        }
        if self.get_numeric_property("volume").is_none() {
            self.set_property_numeric("volume", default_volume);
        }
    }

    pub fn apply_door_role(&mut self, spec: &DoorSpec) {
        self.apply_portal_role(&PortalSpec {
            kind: PortalKind::Door,
            direction: spec.direction.clone(),
            destination: spec.destination.clone(),
            open: spec.open,
            lock_id: spec.lock_id.clone(),
            locked: spec.locked,
            lock_consumable: spec.lock_consumable,
            passable: None,
            transparent: None,
        });
    }

    /// Whether this object has a lock (`lock_id` set) — containers and portals.
    pub fn gate_has_lock(&self) -> bool {
        self.container_lock_id().is_some()
    }

    /// Open state for a portal or container.
    pub fn gate_is_open(&self) -> bool {
        if self.is_portal() {
            return self.get_bool_property("is_open").unwrap_or(false);
        }
        if self.is_container() {
            return self.container_is_open();
        }
        true
    }

    pub fn set_gate_open(&mut self, open: bool) {
        self.set_property_bool("is_open", open);
    }

    pub fn gate_is_locked(&self) -> bool {
        self.gate_has_lock() && self.get_bool_property("is_locked").unwrap_or(false)
    }

    pub fn set_gate_locked(&mut self, locked: bool) {
        self.set_property_bool("is_locked", locked);
    }

    /// Whether `key` opens a lockable gate (container or portal).
    pub fn key_unlocks_gate(key: &Object, gate: &Object) -> bool {
        if !key.is_key() || !gate.gate_has_lock() {
            return false;
        }
        match (key.key_lock_id(), gate.container_lock_id()) {
            (Some(k), Some(c)) => k == c,
            _ => false,
        }
    }

    /// Whether `key` opens `container` (supports one-to-one and shared lock ids).
    pub fn key_unlocks_container(key: &Object, container: &Object) -> bool {
        Object::key_unlocks_gate(key, container)
    }

    /// Items worn on body slots (subset of `body_slots` for wear-type slots).
    pub fn worn_items(&self) -> HashMap<String, ObjectId> {
        self.body_slots()
    }

    pub fn apply_container_role(&mut self, spec: &ContainerSpec) {
        self.set_property_bool("is_container", true);
        self.set_property_int("capacity", i64::from(spec.capacity));
        self.set_property_list("contents", vec![]);
        if let Some(w) = spec.max_weight {
            self.set_property_int("max_weight", w);
        }
        if let Some(v) = spec.max_volume {
            self.set_property_int("max_volume", v);
        }
        self.set_property_bool("is_wearable", spec.wearable);
        if spec.wearable {
            let slot = spec.wear_slot.as_deref().unwrap_or("torso");
            self.set_property_string("wear_slot", slot);
        }
        self.set_property_bool("is_pocketable", false);
        self.set_property_bool("is_open", spec.open);
        if let Some(ref lock_id) = spec.lock_id {
            self.set_container_lock_id(lock_id);
            self.set_container_locked(spec.locked);
            if spec.lock_consumable {
                self.set_property_bool("lock_consumable", true);
            }
        }
        if let Some(ref types) = spec.allowed_types {
            if !types.is_empty() {
                self.set_property_string("allowed_types", types.join(","));
            }
        }
        if self.get_numeric_property("weight").is_none() {
            self.set_property_numeric("weight", 1.0);
        }
        if self.get_numeric_property("volume").is_none() {
            self.set_property_numeric("volume", 1.0);
        }
    }

    pub fn apply_wearable_role(&mut self, spec: &WearableSpec) {
        self.set_property_bool("is_wearable", true);
        self.set_property_string("wear_slot", &spec.wear_slot);
        self.set_property_numeric("weight", spec.weight);
        self.set_property_numeric("volume", spec.volume);
        self.apply_carry_modifiers(spec.mod_max_weight, spec.mod_encumbrance);
        self.apply_equipment_mods(
            spec.mod_max_health,
            spec.stat_mods.clone(),
            spec.skill_mods.clone(),
            spec.grant_effects.clone(),
        );
    }

    /// Apply stat/skill/health modifiers and granted effects (wearable or wielded gear).
    pub fn apply_equipment_mods(
        &mut self,
        mod_max_health: Option<i64>,
        stat_mods: HashMap<String, i64>,
        skill_mods: HashMap<String, i64>,
        grant_effects: Vec<String>,
    ) {
        if let Some(bonus) = mod_max_health.filter(|b| *b != 0) {
            self.set_property_int("mod_max_health", bonus);
        }
        if !stat_mods.is_empty() {
            self.set_int_map("mod_stats", stat_mods);
        }
        if !skill_mods.is_empty() {
            self.set_int_map("mod_skills", skill_mods);
        }
        if !grant_effects.is_empty() {
            self.set_string_list("grant_effects", grant_effects);
        }
    }

    pub fn equipment_max_health_bonus(&self) -> i64 {
        self.get_int_property("mod_max_health").unwrap_or(0)
    }

    pub fn equipment_stat_mods(&self) -> HashMap<String, i64> {
        self.get_int_map("mod_stats")
    }

    pub fn equipment_skill_mods(&self) -> HashMap<String, i64> {
        self.get_int_map("mod_skills")
    }

    pub fn equipment_grant_effects(&self) -> Vec<String> {
        self.get_string_list("grant_effects")
    }

    /// Apply carry-capacity / encumbrance modifiers (wearable equipment).
    pub fn apply_carry_modifiers(
        &mut self,
        max_weight_bonus: Option<i64>,
        encumbrance_factor: Option<f64>,
    ) {
        if let Some(bonus) = max_weight_bonus.filter(|b| *b != 0) {
            self.set_property_int("mod_max_weight", bonus);
        }
        if let Some(factor) = encumbrance_factor.filter(|f| f.is_finite() && (*f - 1.0).abs() > 1e-9)
        {
            self.set_property_numeric("mod_encumbrance", factor);
        }
    }

    /// Bonus added to the wearer's `max_weight` while this item is worn.
    pub fn carry_max_weight_bonus(&self) -> i64 {
        self.get_int_property("mod_max_weight").unwrap_or(0)
    }

    /// Encumbrance multiplier while worn (`1.0` = no change).
    pub fn carry_encumbrance_factor(&self) -> f64 {
        self.get_numeric_property("mod_encumbrance")
            .filter(|f| f.is_finite())
            .unwrap_or(1.0)
    }

    pub fn has_carry_modifiers(&self) -> bool {
        self.carry_max_weight_bonus() != 0
            || (self.carry_encumbrance_factor() - 1.0).abs() > 1e-9
    }

    pub fn apply_item_phys(&mut self, spec: &ItemPhysSpec) {
        self.set_property_numeric("weight", spec.weight);
        self.set_property_numeric("volume", spec.volume);
        self.set_property_bool("is_pocketable", spec.pocketable);
        if !self.has_container_role() {
            self.set_property_bool("is_container", false);
        }
        if !self.has_wearable_role() {
            self.set_property_bool("is_wearable", false);
        }
    }

    pub fn apply_stackable_role(&mut self, spec: &StackableSpec) {
        self.set_property_bool("stackable", true);
        self.set_property_int("stack_count", i64::from(spec.count));
        self.set_property_int("max_stack", i64::from(spec.max_stack));
    }

    /// Initialize a naked player from a MUDL player template (creature role).
    pub fn init_creature_role(&mut self, template: &PlayerTemplate) {
        self.add_property(Property {
            name: "creature".to_string(),
            value: Value::String(template.creature.clone()),
            permissions: PermissionFlags::OWNER,
            behavior: None,
        });
        self.add_property(Property {
            name: "gender".to_string(),
            value: Value::String(template.gender.clone()),
            permissions: PermissionFlags::OWNER,
            behavior: None,
        });
        self.set_property_map("body_slots", HashMap::new());
        self.set_property_int(
            "max_weight",
            crate::object::weight::DEFAULT_PLAYER_MAX_WEIGHT,
        );
    }

    /// Backward-compatible alias for [`init_creature_role`](Self::init_creature_role).
    pub fn init_body(&mut self, template: &PlayerTemplate) {
        self.init_creature_role(template);
    }

    /// Default item properties (backward-compatible with pre-M1 objects).
    pub fn init_item_defaults(&mut self, pocketable: bool) {
        self.init_item_defaults_if_unset(pocketable);
    }

    /// Fill generic item fields only when not already set by a prototype or role.
    pub fn init_item_defaults_if_unset(&mut self, pocketable: bool) {
        if self.get_numeric_property("weight").is_none() {
            self.set_property_numeric("weight", 1.0);
        }
        if self.get_numeric_property("volume").is_none() {
            self.set_property_numeric("volume", 1.0);
        }
        if self.get_bool_property("is_pocketable").is_none() {
            self.set_property_bool("is_pocketable", pocketable);
        }
        if !self.has_container_role() && self.get_property("is_container").is_none() {
            self.set_property_bool("is_container", false);
        }
        if !self.has_wearable_role() && self.get_property("is_wearable").is_none() {
            self.set_property_bool("is_wearable", false);
        }
    }

    /// Default container properties (backward-compatible with pre-M1 objects).
    pub fn init_container_defaults(&mut self, capacity: u32, wearable: bool) {
        self.apply_container_role(&ContainerSpec {
            capacity,
            max_weight: None,
            max_volume: None,
            wearable,
            wear_slot: if wearable {
                Some("torso".to_string())
            } else {
                None
            },
            ..crate::object::ContainerSpec::default()
        });
    }

    pub fn set_property_bool(&mut self, name: &str, value: bool) {
        self.add_property(Property {
            name: name.to_string(),
            value: Value::Bool(value),
            permissions: PermissionFlags::OWNER,
            behavior: None,
        });
    }

    pub fn set_property_int(&mut self, name: &str, value: i64) {
        self.add_property(Property {
            name: name.to_string(),
            value: Value::Int(value),
            permissions: PermissionFlags::OWNER,
            behavior: None,
        });
    }

    pub fn set_property_string(&mut self, name: &str, value: impl Into<String>) {
        self.add_property(Property {
            name: name.to_string(),
            value: Value::String(value.into()),
            permissions: PermissionFlags::OWNER,
            behavior: None,
        });
    }

    pub fn set_property_list(&mut self, name: &str, items: Vec<ObjectId>) {
        self.add_property(Property {
            name: name.to_string(),
            value: Value::List(items.into_iter().map(Value::ObjectRef).collect()),
            permissions: PermissionFlags::OWNER,
            behavior: None,
        });
    }

    pub fn get_bool_property(&self, name: &str) -> Option<bool> {
        self.get_property(name).and_then(|p| {
            if let Value::Bool(b) = &p.value {
                Some(*b)
            } else {
                None
            }
        })
    }

    pub fn get_int_property(&self, name: &str) -> Option<i64> {
        self.get_property(name).and_then(|p| {
            if let Value::Int(n) = &p.value {
                Some(*n)
            } else {
                None
            }
        })
    }

    pub fn get_numeric_property(&self, name: &str) -> Option<f64> {
        self.get_property(name).and_then(|p| match &p.value {
            Value::Int(n) => Some(*n as f64),
            Value::Float(f) => Some(*f),
            _ => None,
        })
    }

    pub fn get_float_property(&self, name: &str) -> Option<f64> {
        self.get_numeric_property(name)
    }

    pub fn set_int_map(&mut self, name: &str, map: HashMap<String, i64>) {
        let value_map: HashMap<String, Value> = map
            .into_iter()
            .map(|(k, v)| (k, Value::Int(v)))
            .collect();
        self.add_property(Property {
            name: name.to_string(),
            value: Value::Map(value_map),
            permissions: PermissionFlags::OWNER,
            behavior: None,
        });
    }

    pub fn get_int_map(&self, name: &str) -> HashMap<String, i64> {
        self.get_property(name)
            .and_then(|p| {
                if let Value::Map(map) = &p.value {
                    Some(
                        map.iter()
                            .filter_map(|(k, v)| {
                                if let Value::Int(n) = v {
                                    Some((k.clone(), *n))
                                } else {
                                    None
                                }
                            })
                            .collect(),
                    )
                } else {
                    None
                }
            })
            .unwrap_or_default()
    }

    pub fn set_string_list(&mut self, name: &str, items: Vec<String>) {
        self.add_property(Property {
            name: name.to_string(),
            value: Value::List(items.into_iter().map(Value::String).collect()),
            permissions: PermissionFlags::OWNER,
            behavior: None,
        });
    }

    pub fn get_string_list(&self, name: &str) -> Vec<String> {
        self.get_property(name)
            .and_then(|p| {
                if let Value::List(items) = &p.value {
                    Some(
                        items
                            .iter()
                            .filter_map(|v| {
                                if let Value::String(s) = v {
                                    Some(s.clone())
                                } else {
                                    None
                                }
                            })
                            .collect(),
                    )
                } else {
                    None
                }
            })
            .unwrap_or_default()
    }

    pub fn set_property_numeric(&mut self, name: &str, value: f64) {
        let stored = if value.fract().abs() < 1e-9
            && value >= i64::MIN as f64
            && value <= i64::MAX as f64
        {
            Value::Int(value.round() as i64)
        } else {
            Value::Float(value)
        };
        self.add_property(Property {
            name: name.to_string(),
            value: stored,
            permissions: PermissionFlags::OWNER,
            behavior: None,
        });
    }

    pub fn get_string_property(&self, name: &str) -> Option<String> {
        self.get_property(name).and_then(|p| {
            if let Value::String(s) = &p.value {
                Some(s.clone())
            } else {
                None
            }
        })
    }

    pub fn get_object_ref_property(&self, name: &str) -> Option<ObjectId> {
        self.get_property(name).and_then(|p| {
            if let Value::ObjectRef(id) = &p.value {
                Some(id.clone())
            } else {
                None
            }
        })
    }

    pub fn set_property_object_ref(&mut self, name: &str, id: ObjectId) {
        self.add_property(Property {
            name: name.to_string(),
            value: Value::ObjectRef(id),
            permissions: PermissionFlags::OWNER,
            behavior: None,
        });
    }

    pub fn get_object_list_property(&self, name: &str) -> Vec<ObjectId> {
        self.get_property(name)
            .and_then(|p| {
                if let Value::List(items) = &p.value {
                    Some(
                        items
                            .iter()
                            .filter_map(|v| {
                                if let Value::ObjectRef(id) = v {
                                    Some(id.clone())
                                } else {
                                    None
                                }
                            })
                            .collect(),
                    )
                } else {
                    None
                }
            })
            .unwrap_or_default()
    }

    pub fn is_container(&self) -> bool {
        self.has_container_role()
    }

    pub fn is_wearable(&self) -> bool {
        self.has_wearable_role()
    }

    /// Whether this object has text the player can read.
    pub fn is_readable(&self) -> bool {
        self.get_bool_property("is_readable").unwrap_or(false)
            || self
                .read_text()
                .is_some_and(|text| !text.trim().is_empty())
    }

    /// Text shown by the `read` command.
    pub fn read_text(&self) -> Option<String> {
        self.get_string_property("read_text")
    }

    /// Whether the player may write on this object (future `write` command).
    pub fn is_writable(&self) -> bool {
        self.get_bool_property("is_writable").unwrap_or(false)
    }

    /// Player-written content; falls back to [`Self::read_text`] when empty.
    pub fn write_text(&self) -> Option<String> {
        self.get_string_property("write_text")
    }

    pub fn set_write_text(&mut self, text: impl Into<String>) {
        self.set_property_string("write_text", text);
        self.set_property_bool("is_writable", true);
    }

    pub fn apply_readable_role(&mut self, spec: &ReadableSpec) {
        self.set_property_bool("is_readable", true);
        self.set_property_string("read_text", &spec.text);
        self.set_property_bool("is_writable", spec.writable);
    }

    pub fn hand_slot(&self) -> Option<String> {
        self.get_string_property("hand_slot")
    }

    pub fn wear_slot(&self) -> Option<String> {
        self.get_string_property("wear_slot")
    }

    pub fn carried_slot(&self) -> Option<String> {
        self.get_string_property("carried_slot")
    }

    pub fn set_carried_slot(&mut self, slot: Option<&str>) {
        if let Some(slot) = slot {
            self.set_property_string("carried_slot", slot);
        } else {
            self.properties.remove("carried_slot");
        }
    }

    pub(crate) fn add_to_list_property(&mut self, prop: &str, id: ObjectId) {
        let mut list = self.get_object_list_property(prop);
        if !list.contains(&id) {
            list.push(id);
            self.set_property_list(prop, list);
        }
    }

    pub(crate) fn remove_from_list_property(&mut self, prop: &str, id: &ObjectId) {
        let list: Vec<ObjectId> = self
            .get_object_list_property(prop)
            .into_iter()
            .filter(|item| item != id)
            .collect();
        self.set_property_list(prop, list);
    }

    /// Sum volume of all objects inside this container.
    pub fn contents_volume(&self, objects: &HashMap<ObjectId, Object>) -> f64 {
        self.container_contents()
            .iter()
            .filter_map(|id| objects.get(id))
            .map(|obj| obj.volume())
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::PermissionFlags;

    fn bare_object(id: &str) -> Object {
        Object {
            id: ObjectId::new(id),
            name: "test".to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: ObjectId::new("player:admin-001"),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        }
    }

    #[test]
    fn container_defaults_to_open() {
        let mut obj = bare_object("item:bag-001");
        obj.apply_container_role(&ContainerSpec::default());
        assert!(obj.container_is_open());
    }

    #[test]
    fn container_can_start_closed() {
        let mut obj = bare_object("item:chest-001");
        obj.apply_container_role(&ContainerSpec {
            open: false,
            ..ContainerSpec::default()
        });
        assert!(!obj.container_is_open());
        obj.set_container_open(true);
        assert!(obj.container_is_open());
    }

    #[test]
    fn consumable_key_and_lock_flags() {
        let mut key = bare_object("item:charm-001");
        key.apply_key_role(&KeySpec::new("oak-whisper").consumable());
        assert!(key.key_consumable());

        let mut oak = bare_object("item:oak-001");
        oak.apply_portal_role(&PortalSpec {
            kind: PortalKind::Door,
            direction: "in".to_string(),
            destination: "haunted-entry".to_string(),
            open: false,
            lock_id: Some("oak-whisper".to_string()),
            locked: true,
            lock_consumable: true,
            passable: None,
            transparent: None,
        });
        assert!(oak.lock_consumable());
    }

    #[test]
    fn container_lock_and_key_matching() {
        let mut chest = bare_object("item:chest-001");
        chest.apply_container_role(&ContainerSpec {
            lock_id: Some("chest-lock".to_string()),
            locked: true,
            open: false,
            ..ContainerSpec::default()
        });
        assert!(chest.container_is_locked());

        let mut key_a = bare_object("item:key-a-001");
        key_a.apply_key_role(&KeySpec::new("chest-lock"));
        let mut key_b = bare_object("item:key-b-001");
        key_b.apply_key_role(&KeySpec::new("chest-lock"));

        assert!(Object::key_unlocks_container(&key_a, &chest));
        assert!(Object::key_unlocks_container(&key_b, &chest));

        let mut wrong = bare_object("item:key-wrong-001");
        wrong.apply_key_role(&KeySpec::new("other-lock"));
        assert!(!Object::key_unlocks_container(&wrong, &chest));
    }

    #[test]
    fn readable_role_sets_text_properties() {
        let mut obj = bare_object("item:note-001");
        obj.apply_readable_role(&ReadableSpec {
            text: "Mind the dark.".to_string(),
            writable: true,
        });
        assert!(obj.is_readable());
        assert_eq!(obj.read_text().as_deref(), Some("Mind the dark."));
        assert!(obj.is_writable());
    }

    #[test]
    fn container_allowed_types_restrict_items() {
        let mut ring = bare_object("item:ring-001");
        ring.name = "Brass Key Ring".to_string();
        ring.apply_container_role(&ContainerSpec {
            capacity: 4,
            allowed_types: Some(vec!["key".to_string()]),
            ..ContainerSpec::default()
        });
        assert_eq!(
            ring.container_allowed_types().as_deref(),
            Some(&["key".to_string()][..])
        );

        let mut key = bare_object("item:key-001");
        key.apply_key_role(&KeySpec::new("demo"));
        let mut blade = bare_object("item:blade-001");

        assert!(ring.container_accepts_item(&key));
        assert!(!ring.container_accepts_item(&blade));
        assert!(key.item_has_type("key"));
        assert!(!blade.item_has_type("key"));
    }

    #[test]
    fn parse_allowed_types_splits_comma_list() {
        assert_eq!(parse_allowed_types("key, token"), vec!["key", "token"]);
    }

    #[test]
    fn container_role_sets_expected_properties() {
        let mut obj = bare_object("item:bag-001");
        obj.apply_container_role(&ContainerSpec {
            capacity: 5,
            max_weight: Some(100),
            max_volume: Some(50),
            wearable: true,
            wear_slot: Some("torso".to_string()),
            ..crate::object::ContainerSpec::default()
        });

        assert!(obj.has_container_role());
        assert!(obj.has_wearable_role());
        assert_eq!(obj.container_capacity(), 5);
        assert_eq!(obj.container_max_weight(), Some(100));
        assert_eq!(obj.wear_slot(), Some("torso".to_string()));
    }

    #[test]
    fn stackable_weight_scales_with_count() {
        let mut obj = bare_object("item:coin-001");
        obj.set_property_int("weight", 2);
        obj.apply_stackable_role(&StackableSpec {
            count: 10,
            max_stack: 99,
        });
        assert!((obj.weight() - 20.0).abs() < f64::EPSILON);
        assert!((obj.unit_weight() - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn init_item_defaults_if_unset_preserves_role_phys() {
        let mut obj = bare_object("item:cloak-001");
        obj.apply_wearable_role(&WearableSpec::new("back", 2.5, 3.0));
        obj.init_item_defaults_if_unset(false);

        assert!((obj.weight() - 2.5).abs() < f64::EPSILON);
        assert!((obj.volume() - 3.0).abs() < f64::EPSILON);
        assert_eq!(obj.get_bool_property("is_pocketable"), Some(false));
    }

    #[test]
    fn role_summary_reflects_active_roles() {
        let mut obj = bare_object("item:pack-001");
        obj.apply_container_role(&ContainerSpec::default());
        let roles = obj.roles();
        assert!(roles.container);
        assert!(!roles.creature);
    }
}
