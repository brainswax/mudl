//! Client-specific formatters for [`GameMessage`](super::message::GameMessage).
//!
//! The router emits semantic messages; formatters adapt them for IRC plain text,
//! Slack mrkdwn/blocks, or future frontends.

use super::message::GameMessage;

/// Adapt semantic game messages to a client-specific output type.
pub trait MessageFormatter {
    type Output: Clone;

    fn format(&self, message: &GameMessage) -> Self::Output;
}

/// IRC / plain-text transport formatter.
#[derive(Debug, Clone, Copy, Default)]
pub struct PlainTextFormatter;

impl MessageFormatter for PlainTextFormatter {
    type Output = String;

    fn format(&self, message: &GameMessage) -> String {
        match message {
            GameMessage::Plain(text) => text.clone(),
            GameMessage::Say { speaker, text } => crate::irc::format_say(speaker, text),
            GameMessage::Emote { speaker, text } => crate::irc::format_emote(speaker, text),
            GameMessage::Tell { from, text } => crate::irc::format_tell(from, text),
            GameMessage::TellSent { to, text } => crate::irc::format_tell_sent(to, text),
            GameMessage::OpenContext { speaker, room, body } => {
                crate::gateway::format_open_context_post(speaker, room, body)
            }
            GameMessage::Arrival { speaker } => crate::irc::format_arrival(speaker),
            GameMessage::Departure { speaker } => crate::irc::format_departure(speaker),
            GameMessage::Ooc { speaker, text } => crate::irc::format_ooc(speaker, text),
        }
    }
}

/// Slack formatter — produces [`SlackFormattedMessage`] with mrkdwn and optional blocks.
#[derive(Debug, Clone, Copy, Default)]
pub struct SlackMessageFormatter;

impl MessageFormatter for SlackMessageFormatter {
    type Output = crate::slack::SlackFormattedMessage;

    fn format(&self, message: &GameMessage) -> Self::Output {
        let text = match message {
            GameMessage::Plain(text) => text.clone(),
            GameMessage::Say { speaker, text } => crate::slack::format_say(speaker, text),
            GameMessage::Emote { speaker, text } => crate::slack::format_emote(speaker, text),
            GameMessage::Tell { from, text } => crate::slack::format_tell(from, text),
            GameMessage::TellSent { to, text } => crate::slack::format_tell_sent(to, text),
            GameMessage::OpenContext { speaker, room, body } => {
                crate::gateway::format_open_context_post(speaker, room, body)
            }
            GameMessage::Arrival { speaker } => crate::slack::format_arrival(speaker),
            GameMessage::Departure { speaker } => crate::slack::format_departure(speaker),
            GameMessage::Ooc { speaker, text } => crate::slack::format_ooc(speaker, text),
        };
        let kind = crate::slack::classify_slack_output("", &text);
        crate::slack::format_slack_message(&text, kind)
    }
}

/// Format every delivery in a plan using the given formatter.
pub fn format_plan<F: MessageFormatter>(
    formatter: &F,
    plan: &super::message::DeliveryPlan,
) -> FormattedPlan<F::Output> {
    let deliveries = plan
        .deliveries
        .iter()
        .map(|d| FormattedDelivery {
            target: d.target.clone(),
            text: formatter.format(&d.message),
        })
        .collect();
    FormattedPlan {
        deliveries,
        presence_sync: plan.presence_sync.clone(),
        persist: plan.persist,
    }
}

/// A delivery plan with client-specific formatted payloads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormattedDelivery<T> {
    pub target: super::message::DeliveryTarget,
    pub text: T,
}

/// Formatted plan ready for transport execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormattedPlan<T> {
    pub deliveries: Vec<FormattedDelivery<T>>,
    pub presence_sync: Option<super::message::PresenceSyncPlan>,
    pub persist: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_formatter_formats_social_messages() {
        let fmt = PlainTextFormatter;
        assert_eq!(
            fmt.format(&GameMessage::Say {
                speaker: "Alice".into(),
                text: "hi".into(),
            }),
            "Alice says, \"hi\""
        );
        assert_eq!(
            fmt.format(&GameMessage::OpenContext {
                speaker: "Alice".into(),
                room: "The Void".into(),
                body: "A dusty room.".into(),
            }),
            "Alice @ The Void:\nA dusty room."
        );
    }
}