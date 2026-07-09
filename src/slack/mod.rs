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
mod format;
mod input;
mod multi_user;
mod presence;
mod server;
mod session;
mod transport;
mod verify;
mod visibility;

pub use bot::SlackBot;
pub use channels::{
    classify_channel, ic_join_notice, login_presence_joins, logout_presence_parts,
    room_channel_name, room_join_notice, room_presence, room_routing_mode, room_thread_presence,
    shared_ic_presence, speech_presence, ChannelTarget, RoomRoutingMode,
};
pub use dispatch::{
    dispatch_command, slack_help_text, DispatchOutcome, PresenceSync, RoomDelivery,
};
pub use config::SlackConfig;
pub use format::{
    classify_slack_output, escape_mrkdwn, format_emote, format_help_text, format_open_chat,
    format_ooc, format_say, format_slack_message, format_tell, format_tell_sent,
    SlackFormattedMessage, SlackOutputKind,
};
pub use events::{
    classify_slack_channel, classify_slack_channel_with_rooms, parse_events_payload,
    SlackChannelKind, SlackEventBody, SlackEventCallback, SlackEventsPayload,
    SlackMessageEvent,
};
pub use multi_user::{
    append_movement_visibility, is_private_tell_line, speaker_display_name_async,
};
pub use input::normalize_slack_command_input;
pub use presence::{encode_notice, encode_thread, parse_recipient, SlackRecipient};
pub use server::{run_events_server, EventsServerState};
pub use session::{
    is_slack_member_id, normalize_slack_user_id, slack_logged_out_help, SlackSessionContext,
    SlackSessionRegistry,
};
pub use transport::{
    OutgoingSlack, SlackFormattedDelivery, SlackTransport, SlackWebTransport,
};
pub use verify::verify_slack_signature;
pub use visibility::{
    all_connected_nicks, connected_speech_audience_async, resolve_connected_user_async,
    slack_look_scope, SLACK_LOOK_SCOPE,
};
pub use crate::irc::CoLocatedPlayer;
pub use crate::transport::{GameTransport, MockTransport};