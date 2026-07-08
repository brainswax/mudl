//! Role-based access control for meta-commands and privileged plain verbs.

use crate::object::{Object, PermissionFlags};

/// Player / builder / wizard capability tiers (see BUILDER.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ActorTier {
    Player = 0,
    Builder = 1,
    Wizard = 2,
}

/// Authorization failure surfaced to players and gateway callers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    ActorNotFound,
    UnknownNick,
    InsufficientTier {
        required: ActorTier,
        actual: ActorTier,
    },
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ActorNotFound => write!(f, "You seem to have lost yourself."),
            Self::UnknownNick => write!(f, "You are not connected to this world."),
            Self::InsufficientTier { required, .. } => {
                write!(f, "{}", tier_denied_message(*required))
            }
        }
    }
}

impl std::error::Error for AuthError {}

impl PermissionFlags {
    pub fn player_default() -> Self {
        Self::OWNER
    }

    pub fn builder_role() -> Self {
        Self::OWNER | Self::BUILDER
    }

    pub fn wizard_role() -> Self {
        Self::OWNER | Self::BUILDER | Self::WIZARD
    }
}

/// Resolve the highest tier granted by object-level permission flags.
pub fn actor_tier(flags: PermissionFlags) -> ActorTier {
    if flags.contains(PermissionFlags::WIZARD) {
        ActorTier::Wizard
    } else if flags.contains(PermissionFlags::BUILDER) {
        ActorTier::Builder
    } else {
        ActorTier::Player
    }
}

pub fn actor_has_tier(flags: PermissionFlags, required: ActorTier) -> bool {
    actor_tier(flags) >= required
}

pub fn tier_denied_message(required: ActorTier) -> &'static str {
    match required {
        ActorTier::Wizard => "You lack the wizard privilege to use that command.",
        ActorTier::Builder => "You lack the builder privilege to use that command.",
        ActorTier::Player => "You cannot do that.",
    }
}

/// Minimum tier for an `@`-stripped meta verb (`@examine` → `"examine"`).
pub fn required_tier_for_meta_verb(verb: &str) -> ActorTier {
    match verb {
        "look" | "examine" | "dump" => ActorTier::Builder,
        _ => ActorTier::Wizard,
    }
}

/// Minimum tier for privileged plain verbs (no `@` prefix).
pub fn required_tier_for_plain_command(cmd: &str, subcommand: Option<&str>) -> Option<ActorTier> {
    match cmd {
        "load" | "save" => Some(ActorTier::Builder),
        "module" => match subcommand {
            Some("reload") | Some("bundle") => Some(ActorTier::Wizard),
            _ => None,
        },
        _ => None,
    }
}

pub fn authorize_meta_command(actor: &Object, verb: &str) -> Result<(), AuthError> {
    let required = required_tier_for_meta_verb(verb);
    let actual = actor_tier(actor.permissions);
    if actual >= required {
        Ok(())
    } else {
        Err(AuthError::InsufficientTier { required, actual })
    }
}

pub fn authorize_plain_command(
    actor: &Object,
    cmd: &str,
    subcommand: Option<&str>,
) -> Result<(), AuthError> {
    let Some(required) = required_tier_for_plain_command(cmd, subcommand) else {
        return Ok(());
    };
    let actual = actor_tier(actor.permissions);
    if actual >= required {
        Ok(())
    } else {
        Err(AuthError::InsufficientTier { required, actual })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::ObjectId;
    use std::collections::HashMap;

    fn actor(flags: PermissionFlags) -> Object {
        let (revision, updated_at) = crate::object::object_persistence_defaults();
        Object {
            id: ObjectId::new("player:test-001"),
            name: "Test".to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: ObjectId::new("player:test-001"),
            permissions: flags,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
            revision,
            updated_at,
        }
    }

    #[test]
    fn actor_tier_prefers_wizard_over_builder() {
        assert_eq!(
            actor_tier(PermissionFlags::WIZARD | PermissionFlags::BUILDER),
            ActorTier::Wizard
        );
        assert_eq!(actor_tier(PermissionFlags::BUILDER), ActorTier::Builder);
        assert_eq!(actor_tier(PermissionFlags::OWNER), ActorTier::Player);
    }

    #[test]
    fn builder_meta_verbs_allow_inspection_only() {
        let builder = actor(PermissionFlags::builder_role());
        assert!(authorize_meta_command(&builder, "examine").is_ok());
        assert!(authorize_meta_command(&builder, "dump").is_ok());
        assert!(authorize_meta_command(&builder, "set").is_err());
    }

    #[test]
    fn wizard_meta_verbs_allow_mutations() {
        let wizard = actor(PermissionFlags::wizard_role());
        assert!(authorize_meta_command(&wizard, "set").is_ok());
        assert!(authorize_meta_command(&wizard, "trigger").is_ok());
        assert!(authorize_meta_command(&wizard, "dig").is_ok());
    }

    #[test]
    fn plain_commands_require_expected_tiers() {
        let player = actor(PermissionFlags::player_default());
        let builder = actor(PermissionFlags::builder_role());
        let wizard = actor(PermissionFlags::wizard_role());

        assert!(authorize_plain_command(&player, "load", None).is_err());
        assert!(authorize_plain_command(&builder, "load", None).is_ok());
        assert!(authorize_plain_command(&builder, "module", Some("reload")).is_err());
        assert!(authorize_plain_command(&wizard, "module", Some("reload")).is_ok());
        assert!(authorize_plain_command(&player, "look", None).is_ok());
    }
}