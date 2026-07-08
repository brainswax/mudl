//! Slack transport and bot integration for multi-user MUDL play (M6).
//!
//! Uses the Slack **Events API** (HTTP request URL) for inbound messages and the
//! Web API for outbound delivery via [`GameTransport`](crate::transport::GameTransport).

mod bot;
mod config;
mod events;
mod input;
mod server;
mod transport;
mod verify;

pub use bot::SlackBot;
pub use config::SlackConfig;
pub use events::{
    classify_slack_channel, parse_events_payload, SlackChannelKind, SlackEventCallback,
    SlackEventsPayload, SlackMessageEvent,
};
pub use crate::transport::{GameTransport, MockTransport};
pub use input::normalize_slack_command_input;
pub use server::{run_events_server, EventsServerState};
pub use transport::{SlackTransport, SlackWebTransport};
pub use verify::verify_slack_signature;