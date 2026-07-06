//! REPL and interactive frontend session state.
//!
//! [`Session`] is the single in-memory authority for one player's world view.
//! Future IRC/gateway frontends can host one session per connection.

pub mod session;

pub use session::{Session, SessionError};