//! Slack transport and bot integration for multi-user MUDL play (M6).
//!
//! Uses the Slack **Events API** (HTTP request URL) for inbound messages and the
//! Web API for outbound delivery via [`GameTransport`](crate::transport::GameTransport).

mod api;
mod bot;
mod channels;
mod config;
mod dispatch;
mod events;
mod input;
mod presence;
mod server;
mod transport;
mod verify;
mod visibility;

pub use bot::SlackBot;
pub use channels::{
    classify_channel, room_channel_name, room_join_notice, room_presence, room_routing_mode,
    room_thread_presence, ChannelTarget, RoomRoutingMode,
};
pub use dispatch::{
    dispatch_command, slack_help_text, DispatchOutcome, PresenceSync, RoomDelivery,
};
pub use config::SlackConfig;
pub use events::{
    classify_slack_channel, parse_events_payload, SlackChannelKind, SlackEventCallback,
    SlackEventsPayload, SlackMessageEvent,
};
pub use input::normalize_slack_command_input;
pub use presence::{encode_notice, encode_thread, parse_recipient, SlackRecipient};
pub use server::{run_events_server, EventsServerState};
pub use transport::{OutgoingSlack, SlackTransport, SlackWebTransport};
pub use verify::verify_slack_signature;
pub use visibility::{resolve_connected_user_async, slack_look_scope, SLACK_LOOK_SCOPE};
pub use crate::irc::CoLocatedPlayer;
pub use crate::transport::{GameTransport, MockTransport};