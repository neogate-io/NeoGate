use chrono::{DateTime, Utc};
use rand::RngExt;
use std::sync::atomic::Ordering;

use crate::{admin::channel::KeySelectionMode, id::DbId};

use super::{
    AttemptedUpstream, ChannelCandidate, KeyCandidate, ModelBlockKey, ModelBlockLookup,
    RoutingCache, UpstreamProtocol,
};

pub(super) struct ChannelAvailability<'a> {
    pub(super) protocol: UpstreamProtocol,
    pub(super) model: &'a str,
    pub(super) now: DateTime<Utc>,
    pub(super) model_blocks: &'a ModelBlockLookup<'a>,
    pub(super) attempted: &'a [AttemptedUpstream],
    pub(super) excluded_endpoint_ids: Option<&'a [DbId]>,
}

pub(super) fn ready_at(cooldown_until: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    cooldown_until.is_none_or(|value| value <= now)
}

pub(super) fn channel_keys<'a>(
    cache: &'a RoutingCache,
    channel: &ChannelCandidate,
) -> &'a [KeyCandidate] {
    cache
        .keys
        .get(&channel.id)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

pub(super) fn channel_matches_model(channel: &ChannelCandidate, model: &str) -> bool {
    channel.models.is_empty() || channel.models.iter().any(|item| item == model)
}

pub(super) fn matching_channel_count(
    cache: &RoutingCache,
    protocol: UpstreamProtocol,
    model: &str,
) -> usize {
    cache
        .channels
        .iter()
        .filter(|channel| channel.protocol == protocol && channel_matches_model(channel, model))
        .count()
}

#[cfg(test)]
pub fn choose_channel(channels: &[ChannelCandidate]) -> Option<ChannelCandidate> {
    if channels.is_empty() {
        return None;
    }
    let highest_priority = channels.iter().map(|item| item.priority).max()?;
    let candidates: Vec<_> = channels
        .iter()
        .filter(|item| item.priority == highest_priority)
        .cloned()
        .collect();
    let total_weight: i32 = candidates.iter().map(|item| item.weight.max(1)).sum();
    let slot = rand::rng().random_range(0..total_weight);
    choose_channel_by_slot(&candidates, slot)
}

pub(super) fn choose_channel_for_request<'a>(
    cache: &'a RoutingCache,
    protocol: UpstreamProtocol,
    model: &str,
    now: DateTime<Utc>,
    model_blocks: &ModelBlockLookup<'_>,
    attempted: &[AttemptedUpstream],
    excluded_endpoint_ids: Option<&[DbId]>,
) -> Option<&'a ChannelCandidate> {
    choose_channel_for_request_matching(
        cache,
        protocol,
        model,
        now,
        model_blocks,
        attempted,
        excluded_endpoint_ids,
        |_| true,
    )
}

pub(super) fn choose_channel_for_request_matching<'a>(
    cache: &'a RoutingCache,
    protocol: UpstreamProtocol,
    model: &str,
    now: DateTime<Utc>,
    model_blocks: &ModelBlockLookup<'_>,
    attempted: &[AttemptedUpstream],
    excluded_endpoint_ids: Option<&[DbId]>,
    matches: impl Fn(&ChannelCandidate) -> bool,
) -> Option<&'a ChannelCandidate> {
    let availability = ChannelAvailability {
        protocol,
        model,
        now,
        model_blocks,
        attempted,
        excluded_endpoint_ids,
    };
    let indexed = cache
        .route_index
        .get(&protocol)
        .and_then(|models| models.get(model))
        .into_iter()
        .chain(cache.wildcard_index.get(&protocol))
        .flat_map(|indexes| indexes.iter().copied())
        .filter_map(|index| cache.channels.get(index));

    let mut highest_priority = None;
    let mut total_weight = 0;
    let mut selected = None;

    for channel in indexed {
        if !matches(channel) || !channel_is_available(cache, channel, &availability) {
            continue;
        }
        let weight = channel.weight.max(1);
        match highest_priority {
            None => {
                highest_priority = Some(channel.priority);
                total_weight = weight;
                selected = Some(channel);
            }
            Some(priority) if channel.priority > priority => {
                highest_priority = Some(channel.priority);
                total_weight = weight;
                selected = Some(channel);
            }
            Some(priority) if channel.priority == priority => {
                total_weight += weight;
                if rand::rng().random_range(0..total_weight) < weight {
                    selected = Some(channel);
                }
            }
            Some(_) => {}
        }
    }

    selected
}

pub(super) fn channel_is_available(
    cache: &RoutingCache,
    channel: &ChannelCandidate,
    availability: &ChannelAvailability<'_>,
) -> bool {
    channel.protocol == availability.protocol
        && channel_matches_model(channel, availability.model)
        && !availability
            .excluded_endpoint_ids
            .is_some_and(|excluded| excluded.contains(&channel.endpoint_id))
        && ready_at(channel.cooldown_until, availability.now)
        && channel_keys(cache, channel).iter().any(|key| {
            key_is_available(
                channel,
                key,
                availability.model,
                availability.now,
                availability.model_blocks,
            ) && !was_attempted(channel, key, availability.attempted)
        })
}

pub(super) fn unavailable_channel_message(
    cache: &RoutingCache,
    protocol: UpstreamProtocol,
    model: &str,
    now: DateTime<Utc>,
    model_blocks: &ModelBlockLookup<'_>,
) -> String {
    let protocol_name = protocol.as_str();
    let protocol_channels: Vec<_> = cache
        .channels
        .iter()
        .filter(|channel| channel.protocol == protocol)
        .collect();

    if protocol_channels.is_empty() {
        let other_protocol_matches: Vec<_> = cache
            .channels
            .iter()
            .filter(|channel| channel.protocol != protocol && channel_matches_model(channel, model))
            .take(3)
            .map(|channel| format!("{} ({})", channel.name, channel.protocol.as_str()))
            .collect();

        if other_protocol_matches.is_empty() {
            return format!(
                "no available {protocol_name} channel for model {model}; add an enabled healthy {protocol_name} channel with an enabled healthy key"
            );
        }

        return format!(
            "no available {protocol_name} channel for model {model}; matching channel(s) use another protocol: {}",
            other_protocol_matches.join(", ")
        );
    }

    let matching_model_channels: Vec<_> = protocol_channels
        .iter()
        .copied()
        .filter(|channel| channel_matches_model(channel, model))
        .collect();
    if matching_model_channels.is_empty() {
        return format!(
            "no available {protocol_name} channel for model {model}; configured {protocol_name} channels do not include this model"
        );
    }

    let ready_channels: Vec<_> = matching_model_channels
        .iter()
        .copied()
        .filter(|channel| ready_at(channel.cooldown_until, now))
        .collect();
    if ready_channels.is_empty() {
        return format!(
            "no available {protocol_name} channel for model {model}; matching channel(s) are cooling down"
        );
    }

    if ready_channels.iter().all(|channel| {
        channel_keys(cache, channel)
            .iter()
            .all(|key| !key_is_available(channel, key, model, now, model_blocks))
    }) {
        return format!(
            "no available {protocol_name} channel for model {model}; matching channel(s) have no enabled healthy key ready to use"
        );
    }

    format!("no available {protocol_name} channel for model {model}")
}

pub(super) fn choose_key<'a>(
    channel: &ChannelCandidate,
    keys: &'a [KeyCandidate],
    model: &str,
    now: DateTime<Utc>,
    model_blocks: &ModelBlockLookup<'_>,
    attempted: &[AttemptedUpstream],
) -> Option<&'a KeyCandidate> {
    let ready_count = keys
        .iter()
        .filter(|key| {
            key_is_available(channel, key, model, now, model_blocks)
                && !was_attempted(channel, key, attempted)
        })
        .count();
    if ready_count == 0 {
        return None;
    }
    let slot = match channel.key_selection_mode {
        KeySelectionMode::Random => rand::rng().random_range(0..ready_count),
        KeySelectionMode::Polling => channel.polling.fetch_add(1, Ordering::Relaxed) % ready_count,
    };
    keys.iter()
        .filter(|key| {
            key_is_available(channel, key, model, now, model_blocks)
                && !was_attempted(channel, key, attempted)
        })
        .nth(slot)
}

pub(super) fn was_attempted(
    channel: &ChannelCandidate,
    key: &KeyCandidate,
    attempted: &[AttemptedUpstream],
) -> bool {
    let channel_key_id = (!channel.use_credentials).then_some(key.id);
    attempted.iter().any(|item| {
        item.channel_id == channel.id
            && item.channel_endpoint_id == channel.endpoint_id
            && item.channel_key_id == channel_key_id
            && item.credential_id == key.credential_id
    })
}

fn key_is_ready(key: &KeyCandidate, now: DateTime<Utc>) -> bool {
    ready_at(key.cooldown_until, now)
}

pub(super) fn key_is_available(
    channel: &ChannelCandidate,
    key: &KeyCandidate,
    model: &str,
    now: DateTime<Utc>,
    model_blocks: &ModelBlockLookup<'_>,
) -> bool {
    key_is_ready(key, now)
        && key_plan_allows_model(channel, key, model)
        && !model_is_blocked(channel, key, model, now, model_blocks)
}

fn key_plan_allows_model(channel: &ChannelCandidate, key: &KeyCandidate, model: &str) -> bool {
    if key.credential_id.is_none() || key.plan_type.is_none() || key.plan_models.is_empty() {
        return true;
    }
    key.plan_models
        .iter()
        .any(|item| item.protocol == channel.protocol && item.model == model)
}

fn model_is_blocked(
    channel: &ChannelCandidate,
    key: &KeyCandidate,
    model: &str,
    now: DateTime<Utc>,
    model_blocks: &ModelBlockLookup<'_>,
) -> bool {
    if model_blocks.is_empty() {
        return false;
    }
    let block_key = ModelBlockKey {
        protocol: channel.protocol,
        endpoint_id: channel.endpoint_id,
        channel_key_id: (!channel.use_credentials).then_some(key.id),
        credential_id: key.credential_id,
        model: model.to_string(),
    };
    model_blocks.contains_active(&block_key, now)
}

#[cfg(test)]
pub fn choose_channel_by_slot(
    channels: &[ChannelCandidate],
    mut slot: i32,
) -> Option<ChannelCandidate> {
    for channel in channels {
        let weight = channel.weight.max(1);
        if slot < weight {
            return Some(channel.clone());
        }
        slot -= weight;
    }
    None
}
