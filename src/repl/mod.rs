//! REPL and interactive frontend session state.
//!
//! - [`WorldState`](crate::world::WorldState) — shared authoritative object graph (M5: `Arc<RwLock<_>>`).
//! - [`PlayerSession`] — per-connection actor and location cache.
//! - [`Session`] — single-user bundle used by the REPL today.

pub mod player_session;
pub mod session;

pub use player_session::PlayerSession;
pub use session::{Session, SessionError};