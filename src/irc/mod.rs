//! IRC transport and bot integration for multi-user MUDL play (M5).
//!
//! Targets **IRCv3-capable servers over TLS** by default (port 6697).

mod bot;
mod capability;
mod channels;
mod config;
mod connect;
mod dispatch;
mod identity;
mod input;
mod message;
mod nick;
mod nickserv;
mod social;
mod transport;
mod visibility;

pub use bot::IrcBot;
pub use capability::{
    cap_end_command, cap_ls_complete, cap_request_command, is_nick_in_use, is_ping,
    is_registration_incomplete, is_welcome,
    registration_commands, registration_error_message, IRCV3_CAPABILITIES,
};
pub use connect::log_outbound_command;
pub use channels::{classify_target, room_channel_name, ChannelTarget};
pub use config::IrcConfig;
pub use connect::{connect, IrcConnection};
pub use dispatch::{dispatch_command, DispatchOutcome};
pub use crate::gateway::{
    parse_login_args, resolve_player_for_login, LoginAuthPolicy, LoginRequest, ParsedLoginArgs,
    LOGIN_TOKEN_PROPERTY,
};
pub use input::normalize_irc_command_input;
pub use identity::{verify_irc_identity, IrcIdentityPolicy};
pub use nickserv::{
    identify_nick_command, parse_nickserv_reply, player_help_text, send_bot_nickserv_bootstrap,
    IrcNickServConfig, NickServNotice,
};
pub use message::{
    format_outgoing, is_bot_echo_privmsg, parse_irc_line, split_ircv3_tags, strip_ircv3_tags,
    IrcMessage, Ircv3Tags,
};
pub use nick::{sanitize_irc_nick, sanitize_nick_display, sanitize_ooc_text, MAX_OOC_TEXT_LEN};
pub use social::{format_emote, format_ooc, format_say, format_tell, format_tell_sent};
pub use visibility::{players_in_room, players_in_room_async, resolve_connected_nick, CoLocatedPlayer};
pub use crate::transport::{GameTransport, MockTransport, OutgoingAction};
pub use transport::{IrcTransport, OutgoingIrc, StreamTransport, TcpIrcClient, TcpTransport};