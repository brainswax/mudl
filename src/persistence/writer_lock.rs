//! Exclusive single-writer lock for file-backed SQLite databases (SEC-23).
//!
//! REPL and IRC each hydrate their own [`WorldState`](crate::world::WorldState). Running
//! both against one database causes split-brain: stale in-memory graphs and conflicting
//! writes. Acquire a process-wide advisory lock before opening the database in production.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use fs2::FileExt;
use serde::{Deserialize, Serialize};

/// Which MUDL front-end holds the database write lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WriterMode {
    Repl,
    Irc,
    Slack,
}

impl WriterMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Repl => "repl",
            Self::Irc => "irc",
            Self::Slack => "slack",
        }
    }
}

/// Policy for enforcing one live writer per database file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriterLockOptions {
    pub enabled: bool,
    pub mode: WriterMode,
}

impl WriterLockOptions {
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            mode: WriterMode::Repl,
        }
    }

    /// Load from environment for production binaries.
    ///
    /// `MUDL_SINGLE_WRITER_ENABLED` — default `true` for file databases.
    /// `MUDL_WRITER_MODE` — `repl`, `irc`, or `slack` (metadata only; lock is exclusive either way).
    pub fn from_env(default_mode: WriterMode) -> Self {
        let enabled = std::env::var("MUDL_SINGLE_WRITER_ENABLED")
            .map(|raw| parse_bool_env(&raw, true))
            .unwrap_or(true);
        let mode = match std::env::var("MUDL_WRITER_MODE")
            .ok()
            .as_deref()
            .map(str::trim)
            .map(str::to_ascii_lowercase)
        {
            Some(mode) if mode == "irc" => WriterMode::Irc,
            Some(mode) if mode == "slack" => WriterMode::Slack,
            Some(mode) if mode == "repl" => WriterMode::Repl,
            _ => default_mode,
        };
        Self { enabled, mode }
    }
}

/// Metadata written into the lock file for operator diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriterLockRecord {
    pub pid: u32,
    pub mode: WriterMode,
    pub database_url: String,
    pub started_at: String,
}

/// RAII exclusive writer lock — released when dropped.
pub struct WriterGuard {
    file: File,
    path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriterLockError {
    pub database_url: String,
    pub lock_path: PathBuf,
    pub holder: Option<WriterLockRecord>,
}

impl std::fmt::Display for WriterLockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "database '{}' is already open for writing",
            self.database_url
        )?;
        if let Some(holder) = &self.holder {
            write!(
                f,
                " ({} mode, pid {}, started {})",
                holder.mode.as_str(),
                holder.pid,
                holder.started_at
            )?;
        }
        write!(
            f,
            ". Only one live writer (REPL, IRC, or Slack) may use the same DATABASE_URL at a time (SEC-23). \
             Stop the other process, or set MUDL_SINGLE_WRITER_ENABLED=0 for local debugging."
        )
    }
}

impl std::error::Error for WriterLockError {}

impl WriterGuard {
    /// Acquire the exclusive writer lock for `database_url`, or return [`WriterLockError`].
    pub fn acquire(database_url: &str, options: &WriterLockOptions) -> Result<Self, WriterLockError> {
        if !options.enabled || is_memory_database(database_url) {
            return Ok(Self::noop());
        }

        let lock_path = lock_path_for_database_url(database_url).ok_or_else(|| WriterLockError {
            database_url: database_url.to_string(),
            lock_path: PathBuf::new(),
            holder: None,
        })?;

        if let Some(parent) = lock_path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|err| WriterLockError {
                    database_url: database_url.to_string(),
                    lock_path: lock_path.clone(),
                    holder: Some(WriterLockRecord {
                        pid: 0,
                        mode: options.mode,
                        database_url: database_url.to_string(),
                        started_at: format!("failed to create lock directory: {err}"),
                    }),
                })?;
            }
        }

        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|err| WriterLockError {
                database_url: database_url.to_string(),
                lock_path: lock_path.clone(),
                holder: Some(WriterLockRecord {
                    pid: 0,
                    mode: options.mode,
                    database_url: database_url.to_string(),
                    started_at: format!("failed to open lock file: {err}"),
                }),
            })?;

        if let Err(err) = file.try_lock_exclusive() {
            let holder = read_lock_record(&lock_path);
            return Err(WriterLockError {
                database_url: database_url.to_string(),
                lock_path,
                holder: holder.or(Some(WriterLockRecord {
                    pid: 0,
                    mode: options.mode,
                    database_url: database_url.to_string(),
                    started_at: format!("lock held by another process ({err})"),
                })),
            });
        }

        let record = WriterLockRecord {
            pid: std::process::id(),
            mode: options.mode,
            database_url: database_url.to_string(),
            started_at: now_rfc3339(),
        };
        write_lock_record(&mut file, &record);

        Ok(Self { file, path: lock_path })
    }

    pub fn lock_path(&self) -> Option<&Path> {
        (!self.path.as_os_str().is_empty()).then_some(self.path.as_path())
    }

    pub fn record(&self) -> Option<WriterLockRecord> {
        self.lock_path().and_then(|path| read_lock_record(path))
    }

    fn noop() -> Self {
        Self {
            file: tempfile_noop_file(),
            path: PathBuf::new(),
        }
    }
}

impl Drop for WriterGuard {
    fn drop(&mut self) {
        let _ = self.file.unlock();
        if !self.path.as_os_str().is_empty() {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

pub fn is_memory_database(database_url: &str) -> bool {
    database_url == ":memory:" || database_url.ends_with(":memory:")
}

pub fn lock_path_for_database_url(database_url: &str) -> Option<PathBuf> {
    if is_memory_database(database_url) {
        return None;
    }
    let path = database_path_from_url(database_url)?;
    Some(PathBuf::from(format!("{}.writer.lock", path.display())))
}

fn database_path_from_url(database_url: &str) -> Option<PathBuf> {
    if is_memory_database(database_url) {
        return None;
    }
    let raw = database_url
        .strip_prefix("sqlite://")
        .or_else(|| database_url.strip_prefix("sqlite:"))
        .unwrap_or(database_url);
    Some(PathBuf::from(raw))
}

fn write_lock_record(file: &mut File, record: &WriterLockRecord) {
    if let Ok(json) = serde_json::to_string_pretty(record) {
        let _ = file.set_len(0);
        let _ = file.write_all(json.as_bytes());
        let _ = file.write_all(b"\n");
        let _ = file.sync_all();
    }
}

fn read_lock_record(path: &Path) -> Option<WriterLockRecord> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(raw.trim()).ok()
}

fn now_rfc3339() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

fn parse_bool_env(raw: &str, default: bool) -> bool {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        _ => default,
    }
}

fn tempfile_noop_file() -> File {
    let path = std::env::temp_dir().join(format!(
        "mudl-writer-noop-{}-{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
        .expect("noop writer lock file")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_db_url(name: &str) -> (String, PathBuf, PathBuf) {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("mudl-writer-lock-{name}-{stamp}"));
        let db_path = dir.join("world.db");
        let url = format!("sqlite:{}", db_path.display());
        let lock_path = lock_path_for_database_url(&url).expect("lock path");
        (url, db_path, lock_path)
    }

    #[test]
    fn memory_database_skips_lock_path() {
        assert!(lock_path_for_database_url(":memory:").is_none());
        assert!(is_memory_database("sqlite::memory:"));
    }

    #[test]
    fn lock_path_derived_from_database_file() {
        let path = lock_path_for_database_url("sqlite://mudl.db").expect("lock path");
        assert_eq!(path, PathBuf::from("mudl.db.writer.lock"));
    }

    #[test]
    fn second_acquire_fails_while_guard_held() {
        let (url, _db_path, lock_path) = temp_db_url("exclusive");
        let options = WriterLockOptions {
            enabled: true,
            mode: WriterMode::Irc,
        };

        let guard = WriterGuard::acquire(&url, &options).expect("first acquire");
        assert_eq!(guard.lock_path(), Some(lock_path.as_path()));

        match WriterGuard::acquire(&url, &options) {
            Err(err) => {
                assert!(err.holder.is_some());
                assert!(err.to_string().contains("SEC-23"));
            }
            Ok(_) => panic!("expected second acquire to fail"),
        }
    }

    #[test]
    fn lock_released_after_drop_allows_reacquire() {
        let (url, _db_path, _) = temp_db_url("reacquire");
        let options = WriterLockOptions {
            enabled: true,
            mode: WriterMode::Repl,
        };

        {
            let _guard = WriterGuard::acquire(&url, &options).expect("first acquire");
        }
        WriterGuard::acquire(&url, &options).expect("reacquire after drop");
    }

    #[test]
    fn disabled_options_skip_exclusive_lock() {
        let (url, _db_path, _) = temp_db_url("disabled");
        let options = WriterLockOptions::disabled();
        let a = WriterGuard::acquire(&url, &options).expect("first");
        let b = WriterGuard::acquire(&url, &options).expect("second");
        assert!(a.lock_path().is_none());
        assert!(b.lock_path().is_none());
    }
}