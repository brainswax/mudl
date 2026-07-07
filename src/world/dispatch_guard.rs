//! Per-world event dispatch stack — depth cap and cycle detection (M5-safe).

use crate::object::ObjectId;

/// Maximum nested `execute_event` depth (discovery → on_discovered → …).
pub const MAX_DISPATCH_DEPTH: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchError {
    DepthExceeded {
        host_id: ObjectId,
        event_name: String,
    },
    CycleDetected {
        host_id: ObjectId,
        event_name: String,
    },
}

impl std::fmt::Display for DispatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DepthExceeded { host_id, event_name } => write!(
                f,
                "event '{event_name}' on {host_id}: dispatch depth exceeded ({MAX_DISPATCH_DEPTH})"
            ),
            Self::CycleDetected { host_id, event_name } => write!(
                f,
                "event '{event_name}' on {host_id}: cycle detected (already in flight)"
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DispatchFrame {
    host_id: ObjectId,
    event_name: String,
}

/// Re-entrant event guard stored on [`WorldState`](crate::world::WorldState), not thread-local.
#[derive(Debug, Default)]
pub struct DispatchStack {
    frames: Vec<DispatchFrame>,
}

impl DispatchStack {
    /// Attempt to enter a dispatch frame.
    pub fn enter(
        &mut self,
        host_id: &ObjectId,
        event_name: &str,
    ) -> Result<DispatchGuard<'_>, DispatchError> {
        if self.frames.len() >= MAX_DISPATCH_DEPTH {
            return Err(DispatchError::DepthExceeded {
                host_id: host_id.clone(),
                event_name: event_name.to_string(),
            });
        }
        if self.frames.iter().any(|frame| {
            frame.host_id == *host_id && frame.event_name == event_name
        }) {
            return Err(DispatchError::CycleDetected {
                host_id: host_id.clone(),
                event_name: event_name.to_string(),
            });
        }
        self.frames.push(DispatchFrame {
            host_id: host_id.clone(),
            event_name: event_name.to_string(),
        });
        Ok(DispatchGuard { stack: self })
    }
}

/// RAII guard — pops the frame on drop.
pub struct DispatchGuard<'a> {
    stack: &'a mut DispatchStack,
}

impl Drop for DispatchGuard<'_> {
    fn drop(&mut self) {
        self.stack.frames.pop();
    }
}

#[cfg(test)]
impl DispatchStack {
    /// Seed the stack without holding a guard (cycle/depth tests only).
    pub fn test_seed(&mut self, host_id: ObjectId, event_name: &str) {
        self.frames.push(DispatchFrame {
            host_id,
            event_name: event_name.to_string(),
        });
    }
}