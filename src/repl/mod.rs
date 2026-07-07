//! REPL and interactive frontend session state.
//!
//! - [`WorldState`](crate::world::WorldState) — shared authoritative object graph.
//! - [`SharedWorld`](crate::world::SharedWorld) — `Arc<Mutex<WorldState>>` for multi-connection hosts.
//! - [`PlayerSession`] — per-connection actor, location cache, and [`PlayerPrefs`].
//! - [`Session`] — REPL bundle (`SharedWorld` + `PlayerSession`); IRC holds one world, many players.

pub mod player_session;
pub mod session;

pub use crate::world::{SharedWorld, WorldState};
pub use player_session::{PlayerPrefs, PlayerSession};
pub use session::{Session, SessionError};