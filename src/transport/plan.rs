//! Convert a routed [`DeliveryPlan`] into formatted transport outcomes.

use super::formatter::{format_plan, PlainTextFormatter};
use super::message::{DeliveryPlan, DeliveryTarget, PresenceSyncPlan};

/// Shared conversion from a formatted plan to outcome field vectors.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OutcomeFields {
    pub to_sender: Vec<String>,
    pub private: Vec<(String, String)>,
    pub room_audience: Vec<(Vec<String>, Vec<String>)>,
    pub channel: Vec<(String, String)>,
    pub presence_sync: Option<PresenceSyncPlan>,
    pub persist: bool,
}

pub fn outcome_fields_from_plan(plan: &DeliveryPlan) -> OutcomeFields {
    let formatted = format_plan(&PlainTextFormatter, plan);
    let mut fields = OutcomeFields {
        persist: formatted.persist,
        presence_sync: formatted.presence_sync.clone(),
        ..Default::default()
    };

    for delivery in formatted.deliveries {
        match delivery.target {
            DeliveryTarget::Actor => fields.to_sender.push(delivery.text),
            DeliveryTarget::User(user) => fields.private.push((user, delivery.text)),
            DeliveryTarget::RoomAudience(audience) => {
                fields
                    .room_audience
                    .push((audience, vec![delivery.text]));
            }
            DeliveryTarget::SharedPresence(presence) => {
                fields.channel.push((presence, delivery.text));
            }
        }
    }

    fields
}