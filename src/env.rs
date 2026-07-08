//! Reliable project `.env` loading for all MUDL binaries.

use std::path::{Path, PathBuf};

use tracing::{debug, warn};

fn project_env_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(".env")
}

/// Load `.env` from the project root (`CARGO_MANIFEST_DIR`) with override semantics.
///
/// Uses manifest-relative path so `cargo run` works regardless of cwd.
/// `.env` values override any existing process environment for loaded keys.
pub fn load_project_env() {
    let manifest_env = project_env_path();
    if manifest_env.is_file() {
        match dotenvy::from_path_override(&manifest_env) {
            Ok(()) => debug!(path = %manifest_env.display(), "loaded .env (override)"),
            Err(err) => warn!(
                path = %manifest_env.display(),
                error = %err,
                "failed to load .env"
            ),
        }
        return;
    }

    if dotenvy::dotenv_override().is_ok() {
        debug!("loaded .env from cwd/parent (override)");
    }
}

/// Read a secret env var: trim whitespace and strip optional surrounding quotes.
pub fn read_secret_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|raw| strip_env_quotes(raw.trim()))
        .filter(|s| !s.is_empty())
}

/// Read a secret directly from `.env` without `$` variable substitution.
///
/// Dotenv parsers expand `$VAR` inside values; passwords containing `$` must be read
/// literally from the file (prefer single quotes in `.env`: `KEY='pa$$word'`).
pub fn read_literal_dotenv_secret(key: &str) -> Option<String> {
    let path = project_env_path();
    let content = std::fs::read_to_string(&path).ok()?;
    parse_literal_dotenv_value(&content, key)
}

/// Prefer literal `.env` parse, then process environment (after `load_project_env`).
pub fn read_config_secret(key: &str) -> Option<String> {
    read_literal_dotenv_secret(key)
        .filter(|s| !s.is_empty())
        .or_else(|| read_secret_env(key))
}

fn parse_literal_dotenv_value(content: &str, key: &str) -> Option<String> {
    for line in content.lines() {
        let mut line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("export ") {
            line = rest.trim();
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        if k.trim() != key {
            continue;
        }
        let value = parse_literal_value(v.trim());
        return if value.is_empty() {
            None
        } else {
            Some(value)
        };
    }
    None
}

fn parse_literal_value(raw: &str) -> String {
    if raw.len() >= 2 && raw.starts_with('\'') {
        if let Some(end) = raw[1..].find('\'') {
            return raw[1..end + 1].to_string();
        }
    }
    if raw.len() >= 2 && raw.starts_with('"') {
        if let Some(end) = raw[1..].find('"') {
            return raw[1..end + 1].to_string();
        }
    }
    raw.to_string()
}

fn strip_env_quotes(raw: &str) -> String {
    if raw.len() >= 2 {
        if raw.starts_with('\'') && raw.ends_with('\'') {
            return raw[1..raw.len() - 1].to_string();
        }
        if raw.starts_with('"') && raw.ends_with('"') {
            return raw[1..raw.len() - 1].to_string();
        }
    }
    raw.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn strip_env_quotes_removes_wrappers() {
        assert_eq!(strip_env_quotes("'sekrit'"), "sekrit");
        assert_eq!(strip_env_quotes("\"sekrit\""), "sekrit");
        assert_eq!(strip_env_quotes("sekrit"), "sekrit");
    }

    #[test]
    fn literal_parser_preserves_dollar_in_single_quotes() {
        let content = "IRC_NICKSERV_PASSWORD='*16zPc1t0yqpB$Zo'\n";
        assert_eq!(
            parse_literal_dotenv_value(content, "IRC_NICKSERV_PASSWORD").as_deref(),
            Some("*16zPc1t0yqpB$Zo")
        );
    }

    #[test]
    fn literal_parser_preserves_dollar_unquoted() {
        let content = "IRC_NICKSERV_PASSWORD=*16zPc1t0yqpB$Zo\n";
        assert_eq!(
            parse_literal_dotenv_value(content, "IRC_NICKSERV_PASSWORD").as_deref(),
            Some("*16zPc1t0yqpB$Zo")
        );
    }

    #[test]
    fn dotenv_loads_nickserv_password_with_dollar_sign_when_single_quoted() {
        let dir = tempfile::tempdir().expect("tempdir");
        let env_file = dir.path().join(".env");
        writeln!(
            std::fs::File::create(&env_file).expect("create"),
            "IRC_NICKSERV_PASSWORD='*16zPc1t0yqpB$Zo'"
        )
        .expect("write");
        let content = std::fs::read_to_string(&env_file).expect("read");
        assert_eq!(
            parse_literal_dotenv_value(&content, "IRC_NICKSERV_PASSWORD").as_deref(),
            Some("*16zPc1t0yqpB$Zo")
        );
    }

    #[test]
    fn read_secret_env_rejects_empty_override() {
        std::env::set_var("IRC_NICKSERV_PASSWORD", "   ");
        assert!(read_secret_env("IRC_NICKSERV_PASSWORD").is_none());
        std::env::remove_var("IRC_NICKSERV_PASSWORD");
    }

    #[test]
    fn project_dotenv_configures_nickserv_when_present() {
        let manifest_env = project_env_path();
        if !manifest_env.is_file() {
            return;
        }
        assert!(
            read_config_secret("IRC_NICKSERV_PASSWORD").is_some(),
            "IRC_NICKSERV_PASSWORD should load from project .env"
        );
    }
}