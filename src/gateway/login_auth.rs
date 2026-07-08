//! Transport login authentication — tokens, identity bindings, and player resolution (SEC-01).

use std::collections::HashMap;

use crate::object::{Object, ObjectId};

/// Property on player objects holding a shared-secret login token (set via `@set` or MUDL).
pub const LOGIN_TOKEN_PROPERTY: &str = "login_token";

/// Policy for binding transport identities (IRC nick, Slack user id) to player actors.
#[derive(Debug, Clone, PartialEq)]
pub struct LoginAuthPolicy {
    /// When true, a valid token (and optional identity binding) is required before `SessionManager::login`.
    pub require_auth: bool,
    /// Transport identity (lowercase) → player id. Restricts which actor an identity may claim.
    pub identity_bindings: HashMap<String, ObjectId>,
    /// Player id → token from environment (`MUDL_LOGIN_TOKENS`). Overrides object property when both set.
    pub env_tokens: HashMap<String, String>,
}

impl Default for LoginAuthPolicy {
    fn default() -> Self {
        Self::permissive()
    }
}

impl LoginAuthPolicy {
    /// Open login for local dev and unit tests (no token).
    pub fn permissive() -> Self {
        Self {
            require_auth: false,
            identity_bindings: HashMap::new(),
            env_tokens: HashMap::new(),
        }
    }

    /// Load policy from environment (used by live IRC / future Slack transports).
    pub fn from_env() -> Self {
        let require_auth = match std::env::var("MUDL_LOGIN_REQUIRE_AUTH") {
            Ok(raw) => parse_bool_env(&raw, true),
            Err(_) => std::env::var("IRC_MOCK").is_err(),
        };
        Self {
            require_auth,
            identity_bindings: parse_identity_bindings(
                std::env::var("MUDL_LOGIN_IDENTITY_BINDINGS").ok().as_deref(),
            ),
            env_tokens: parse_env_tokens(std::env::var("MUDL_LOGIN_TOKENS").ok().as_deref()),
        }
    }

    pub fn logged_out_help(&self) -> String {
        if self.require_auth {
            "Send 'login <token>' or 'login <player-id> <token>'. Ask your operator for credentials."
                .to_string()
        } else {
            "Send 'login' to bind your nick to a matching player name, or 'login <player-id>'."
                .to_string()
        }
    }
}

/// Parsed `login` command arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedLoginArgs {
    pub explicit_player_id: Option<String>,
    pub token: Option<String>,
}

/// Credentials presented at login time.
#[derive(Debug, Clone, PartialEq)]
pub struct LoginRequest<'a> {
    pub transport: &'a str,
    pub identity: &'a str,
    pub player_id: &'a ObjectId,
    pub token: Option<&'a str>,
    pub player: &'a Object,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginAuthError {
    AuthenticationFailed,
}

impl std::fmt::Display for LoginAuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AuthenticationFailed => write!(f, "Invalid login credentials."),
        }
    }
}

impl std::error::Error for LoginAuthError {}

/// Parse `login` arguments into an optional explicit player id and token.
pub fn parse_login_args(args: &[String]) -> ParsedLoginArgs {
    match args.len() {
        0 => ParsedLoginArgs {
            explicit_player_id: None,
            token: None,
        },
        1 => {
            let first = args[0].as_str();
            if first.starts_with("player:") {
                ParsedLoginArgs {
                    explicit_player_id: Some(first.to_string()),
                    token: None,
                }
            } else {
                ParsedLoginArgs {
                    explicit_player_id: None,
                    token: Some(first.to_string()),
                }
            }
        }
        _ => {
            if args[0].starts_with("player:") {
                ParsedLoginArgs {
                    explicit_player_id: Some(args[0].clone()),
                    token: Some(args[1..].join(" ")),
                }
            } else {
                ParsedLoginArgs {
                    explicit_player_id: None,
                    token: Some(args.join(" ")),
                }
            }
        }
    }
}

/// Resolve a player for login: explicit id, token lookup, or nick name match (open mode only).
pub fn resolve_player_for_login(
    identity: &str,
    parsed: &ParsedLoginArgs,
    policy: &LoginAuthPolicy,
    objects: &HashMap<ObjectId, Object>,
) -> Option<ObjectId> {
    if let Some(raw) = parsed.explicit_player_id.as_deref() {
        let id = ObjectId::new(raw);
        return objects.get(&id).is_some().then_some(id);
    }

    if let Some(token) = parsed.token.as_deref() {
        if let Some(id) = resolve_player_by_token(token, policy, objects) {
            return Some(id);
        }
    }

    if policy.require_auth {
        return None;
    }

    objects
        .values()
        .filter(|obj| obj.id.as_str().starts_with("player:"))
        .find(|obj| obj.name.eq_ignore_ascii_case(identity))
        .map(|obj| obj.id.clone())
}

/// Find the player object id for a login token (env map or `login_token` property).
pub fn resolve_player_by_token(
    token: &str,
    policy: &LoginAuthPolicy,
    objects: &HashMap<ObjectId, Object>,
) -> Option<ObjectId> {
    let mut matches = Vec::new();

    for (player_id, env_token) in &policy.env_tokens {
        if constant_time_eq(token, env_token) {
            matches.push(ObjectId::new(player_id));
        }
    }

    for obj in objects.values() {
        if !obj.id.as_str().starts_with("player:") {
            continue;
        }
        if policy.env_tokens.contains_key(obj.id.as_str()) {
            continue;
        }
        if let Some(prop_token) = obj.get_string_property(LOGIN_TOKEN_PROPERTY) {
            if constant_time_eq(token, &prop_token) {
                matches.push(obj.id.clone());
            }
        }
    }

    if matches.len() == 1 {
        matches.pop()
    } else {
        None
    }
}

/// Verify identity binding and token before attaching a session.
pub fn verify_login(policy: &LoginAuthPolicy, request: LoginRequest<'_>) -> Result<(), LoginAuthError> {
    if !policy.require_auth {
        return Ok(());
    }

    let identity_key = normalize_identity(request.identity);

    match policy.identity_bindings.get(&identity_key) {
        Some(bound) if bound != request.player_id => {
            return Err(LoginAuthError::AuthenticationFailed);
        }
        None if !policy.identity_bindings.is_empty() => {
            return Err(LoginAuthError::AuthenticationFailed);
        }
        _ => {}
    }

    let Some(token) = request.token else {
        return Err(LoginAuthError::AuthenticationFailed);
    };

    let expected = expected_token(request.player_id, request.player, policy);
    let Some(expected) = expected else {
        return Err(LoginAuthError::AuthenticationFailed);
    };

    if constant_time_eq(token, &expected) {
        Ok(())
    } else {
        Err(LoginAuthError::AuthenticationFailed)
    }
}

fn expected_token(player_id: &ObjectId, player: &Object, policy: &LoginAuthPolicy) -> Option<String> {
    if let Some(token) = policy.env_tokens.get(player_id.as_str()) {
        return Some(token.clone());
    }
    player.get_string_property(LOGIN_TOKEN_PROPERTY)
}

fn normalize_identity(identity: &str) -> String {
    identity.trim().to_ascii_lowercase()
}

fn parse_bool_env(raw: &str, default: bool) -> bool {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        _ => default,
    }
}

/// `MUDL_LOGIN_TOKENS=player:hero-001=secret,player:hero-002=other`
fn parse_env_tokens(raw: Option<&str>) -> HashMap<String, String> {
    let Some(raw) = raw else {
        return HashMap::new();
    };
    let mut out = HashMap::new();
    for entry in raw.split(',') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let Some((player_id, token)) = entry.split_once('=') else {
            continue;
        };
        let player_id = player_id.trim();
        let token = token.trim();
        if !player_id.is_empty() && !token.is_empty() {
            out.insert(player_id.to_string(), token.to_string());
        }
    }
    out
}

/// `MUDL_LOGIN_IDENTITY_BINDINGS=alice=player:hero-001,bob=player:hero-002`
fn parse_identity_bindings(raw: Option<&str>) -> HashMap<String, ObjectId> {
    let Some(raw) = raw else {
        return HashMap::new();
    };
    let mut out = HashMap::new();
    for entry in raw.split(',') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let Some((identity, player_id)) = entry.split_once('=') else {
            continue;
        };
        let identity = normalize_identity(identity);
        let player_id = player_id.trim();
        if !identity.is_empty() && !player_id.is_empty() {
            out.insert(identity, ObjectId::new(player_id));
        }
    }
    out
}

/// Constant-time string comparison to reduce timing leaks on token checks.
fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.bytes().zip(b.bytes()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::PermissionFlags;

    fn player(id: &str, name: &str, token: Option<&str>) -> Object {
        let mut obj = Object {
            id: ObjectId::new(id),
            name: name.to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: ObjectId::new(id),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            revision: 0,
            updated_at: None,
            is_deleted: false,
            deleted_at: None,
        };
        if let Some(token) = token {
            obj.set_property_string(LOGIN_TOKEN_PROPERTY, token);
        }
        obj
    }

    fn objects(entries: &[(&str, &str, Option<&str>)]) -> HashMap<ObjectId, Object> {
        entries
            .iter()
            .map(|(id, name, token)| {
                let obj = player(id, name, *token);
                (obj.id.clone(), obj)
            })
            .collect()
    }

    #[test]
    fn parse_login_args_explicit_and_token() {
        assert_eq!(
            parse_login_args(&["player:hero-001".to_string(), "secret".to_string()]),
            ParsedLoginArgs {
                explicit_player_id: Some("player:hero-001".to_string()),
                token: Some("secret".to_string()),
            }
        );
        assert_eq!(
            parse_login_args(&["mytoken".to_string()]),
            ParsedLoginArgs {
                explicit_player_id: None,
                token: Some("mytoken".to_string()),
            }
        );
    }

    #[test]
    fn permissive_allows_name_match_without_token() {
        let policy = LoginAuthPolicy::permissive();
        let objs = objects(&[("player:hero-001", "Alice", None)]);
        let parsed = ParsedLoginArgs {
            explicit_player_id: None,
            token: None,
        };
        let id = resolve_player_for_login("alice", &parsed, &policy, &objs);
        assert_eq!(id.as_ref().map(|i| i.as_str()), Some("player:hero-001"));
    }

    #[test]
    fn require_auth_denies_name_only_login() {
        let policy = LoginAuthPolicy {
            require_auth: true,
            ..LoginAuthPolicy::permissive()
        };
        let objs = objects(&[("player:hero-001", "Alice", Some("sekrit"))]);
        let parsed = ParsedLoginArgs {
            explicit_player_id: None,
            token: None,
        };
        assert!(resolve_player_for_login("alice", &parsed, &policy, &objs).is_none());
    }

    #[test]
    fn token_login_resolves_player() {
        let policy = LoginAuthPolicy {
            require_auth: true,
            env_tokens: HashMap::from([(
                "player:hero-001".to_string(),
                "sekrit".to_string(),
            )]),
            ..LoginAuthPolicy::permissive()
        };
        let objs = objects(&[("player:hero-001", "Alice", None)]);
        let parsed = ParsedLoginArgs {
            explicit_player_id: None,
            token: Some("sekrit".to_string()),
        };
        let id = resolve_player_for_login("any-nick", &parsed, &policy, &objs);
        assert_eq!(id.as_ref().map(|i| i.as_str()), Some("player:hero-001"));
    }

    #[test]
    fn verify_login_checks_identity_binding() {
        let policy = LoginAuthPolicy {
            require_auth: true,
            identity_bindings: HashMap::from([(
                "alice".to_string(),
                ObjectId::new("player:hero-001"),
            )]),
            env_tokens: HashMap::from([(
                "player:hero-001".to_string(),
                "sekrit".to_string(),
            )]),
            ..LoginAuthPolicy::permissive()
        };
        let objs = objects(&[("player:hero-001", "Alice", None)]);
        let hero = &objs[&ObjectId::new("player:hero-001")];

        let ok = verify_login(
            &policy,
            LoginRequest {
                transport: "irc",
                identity: "alice",
                player_id: &ObjectId::new("player:hero-001"),
                token: Some("sekrit"),
                player: hero,
            },
        );
        assert!(ok.is_ok());

        let bad_nick = verify_login(
            &policy,
            LoginRequest {
                transport: "irc",
                identity: "eve",
                player_id: &ObjectId::new("player:hero-001"),
                token: Some("sekrit"),
                player: hero,
            },
        );
        assert_eq!(bad_nick, Err(LoginAuthError::AuthenticationFailed));

        let wrong_player = verify_login(
            &policy,
            LoginRequest {
                transport: "irc",
                identity: "alice",
                player_id: &ObjectId::new("player:hero-002"),
                token: Some("sekrit"),
                player: hero,
            },
        );
        assert_eq!(wrong_player, Err(LoginAuthError::AuthenticationFailed));
    }

    #[test]
    fn verify_login_rejects_wrong_token() {
        let policy = LoginAuthPolicy {
            require_auth: true,
            env_tokens: HashMap::from([(
                "player:hero-001".to_string(),
                "sekrit".to_string(),
            )]),
            ..LoginAuthPolicy::permissive()
        };
        let objs = objects(&[("player:hero-001", "Alice", None)]);
        let hero = &objs[&ObjectId::new("player:hero-001")];
        let result = verify_login(
            &policy,
            LoginRequest {
                transport: "irc",
                identity: "alice",
                player_id: &ObjectId::new("player:hero-001"),
                token: Some("wrong"),
                player: hero,
            },
        );
        assert_eq!(result, Err(LoginAuthError::AuthenticationFailed));
    }

    #[test]
    fn parse_env_tokens_and_bindings() {
        let tokens = parse_env_tokens(Some("player:a=one,player:b=two"));
        assert_eq!(tokens.get("player:a"), Some(&"one".to_string()));
        let bindings = parse_identity_bindings(Some("Alice=player:a,Bob=player:b"));
        assert_eq!(
            bindings.get("alice"),
            Some(&ObjectId::new("player:a"))
        );
    }
}