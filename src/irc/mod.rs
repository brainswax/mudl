//! IRC transport and bot integration for multi-user MUDL play (M5).
//!
//! Targets **IRCv3-capable servers over TLS** by default (port 6697).

mod bot;
mod capability;
mod channels;
mod config;
mod connect;
mod dispatch;
mod message;
mod social;
mod transport;
mod visibility;

pub use bot::IrcBot;
pub use capability::{
    cap_end_command, cap_ls_complete, cap_request_command, is_welcome, registration_commands,
    IRCV3_CAPABILITIES,
};
pub use channels::{classify_target, room_channel_name, ChannelTarget};
pub use config::IrcConfig;
pub use connect::{connect, IrcConnection};
pub use dispatch::{dispatch_command, resolve_player_for_login, DispatchOutcome};
pub use message::{format_outgoing, parse_irc_line, IrcMessage};
pub use social::{format_emote, format_ooc, format_say, format_tell};
pub use transport::{IrcTransport, MockTransport, OutgoingIrc, StreamTransport, TcpIrcClient, TcpTransport};
pub use visibility::{players_in_room, resolve_connected_nick};