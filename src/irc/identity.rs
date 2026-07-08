//! Optional IRC identity verification — account-tag and nick bindings (SEC-03).

use std::collections::HashMap;

/// Policy for trusting wire nicks beyond MUDL login tokens.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct IrcIdentityPolicy {
    /// Reject PRIVMSG when the server omits `account-tag` or sends `account=*`.
    pub require_account_tag: bool,
    /// Normalized IRC nick → required SASL/account name from `account-tag`.
    pub account_bindings: HashMap<String, String>,
}

impl IrcIdentityPolicy {
    pub fn from_env() -> Self {
        Self {
            require_account_tag: parse_bool_env(
                std::env::var("IRC_REQUIRE_ACCOUNT_TAG").ok().as_deref(),
                false,
            ),
            account_bindings: parse_account_bindings(
                std::env::var("MUDL_IRC_ACCOUNT_BINDINGS").ok().as_deref(),
            ),
        }
    }

    pub fn is_strict(&self) -> bool {
        self.require_account_tag || !self.account_bindings.is_empty()
    }
}

/// Verify IRC `account-tag` against operator policy before accepting commands/OOC.
pub fn verify_irc_identity(
    nick: &str,
    account: Option<&str>,
    policy: &IrcIdentityPolicy,
) -> Result<(), String> {
    if !policy.is_strict() {
        return Ok(());
    }

    let account = account.map(str::trim).filter(|a| !a.is_empty() && *a != "*");

    if policy.require_account_tag && account.is_none() {
        return Err(
            "This network requires a registered/SASL-identified IRC account. \
             Identify to NickServ or use SASL, then try again."
                .to_string(),
        );
    }

    if let Some(required) = policy.account_bindings.get(nick) {
        match account {
            Some(actual) if actual.eq_ignore_ascii_case(required) => Ok(()),
            _ => Err(format!(
                "IRC account verification failed for nick '{nick}'. \
                 Contact the operator if you believe this is an error."
            )),
        }
    } else if policy.require_account_tag {
        Ok(())
    } else {
        Ok(())
    }
}

fn parse_bool_env(raw: Option<&str>, default: bool) -> bool {
    match raw.map(str::trim).map(|s| s.to_ascii_lowercase()) {
        Some(ref s) if s == "1" || s == "true" || s == "yes" || s == "on" => true,
        Some(ref s) if s == "0" || s == "false" || s == "no" || s == "off" => false,
        Some(_) => default,
        None => default,
    }
}

/// `MUDL_IRC_ACCOUNT_BINDINGS=alice=MyAccount,bob=OtherAccount`
fn parse_account_bindings(raw: Option<&str>) -> HashMap<String, String> {
    let Some(raw) = raw else {
        return HashMap::new();
    };
    let mut out = HashMap::new();
    for entry in raw.split(',') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let Some((nick, account)) = entry.split_once('=') else {
            continue;
        };
        let nick = nick.trim().to_ascii_lowercase();
        let account = account.trim();
        if !nick.is_empty() && !account.is_empty() {
            out.insert(nick, account.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn require_account_tag_rejects_unidentified() {
        let policy = IrcIdentityPolicy {
            require_account_tag: true,
            ..Default::default()
        };
        assert!(verify_irc_identity("alice", None, &policy).is_err());
        assert!(verify_irc_identity("alice", Some("*"), &policy).is_err());
        assert!(verify_irc_identity("alice", Some("AliceAccount"), &policy).is_ok());
    }

    #[test]
    fn account_bindings_enforce_per_nick() {
        let policy = IrcIdentityPolicy {
            account_bindings: HashMap::from([(
                "alice".to_string(),
                "AliceAccount".to_string(),
            )]),
            ..Default::default()
        };
        assert!(verify_irc_identity("alice", Some("AliceAccount"), &policy).is_ok());
        assert!(verify_irc_identity("alice", Some("wrong"), &policy).is_err());
        assert!(verify_irc_identity("bob", None, &policy).is_ok());
    }

    #[test]
    fn permissive_policy_allows_all() {
        let policy = IrcIdentityPolicy::default();
        assert!(verify_irc_identity("alice", None, &policy).is_ok());
    }
}