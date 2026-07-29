use super::*;
use std::sync::LazyLock;

static EMPTY_BLOCKS: LazyLock<HashMap<ModelBlockKey, DateTime<Utc>>> = LazyLock::new(HashMap::new);
static EMPTY_LOCAL_BLOCKS: LazyLock<ModelBlockCache> = LazyLock::new(ModelBlockCache::default);

fn empty_block_lookup() -> ModelBlockLookup<'static> {
    ModelBlockLookup::new(&EMPTY_BLOCKS, &EMPTY_LOCAL_BLOCKS)
}

fn candidate(name: &str, priority: i32, weight: i32, models: Vec<&str>) -> ChannelCandidate {
    ChannelCandidate {
        id: 1,
        endpoint_id: 10,
        protocol: UpstreamProtocol::Openai,
        provider: "openai".to_string(),
        name: name.to_string(),
        base_url: "https://example.com".to_string(),
        models: models.into_iter().map(str::to_string).collect(),
        priority,
        weight,
        key_selection_mode: KeySelectionMode::Polling,
        use_credentials: false,
        cooldown_until: None,
        adapter_hint: None,
        polling: Arc::new(AtomicUsize::new(0)),
    }
}

#[test]
fn model_match_accepts_empty_model_list() {
    let channel = candidate("any", 0, 1, vec![]);
    assert!(channel_matches_model(&channel, "gpt-4.1"));
}

#[test]
fn model_match_requires_exact_listed_model() {
    let channel = candidate("strict", 0, 1, vec!["gpt-4.1"]);
    assert!(channel_matches_model(&channel, "gpt-4.1"));
    assert!(!channel_matches_model(&channel, "gpt-4o-mini"));
}

#[test]
fn route_indexes_keep_wildcard_channels_reachable() {
    let wildcard = candidate("any", 0, 1, vec![]);
    let strict = candidate("strict", 0, 1, vec!["gpt-4.1"]);
    let (route_index, wildcard_index) = build_route_indexes(&[wildcard, strict]);

    assert_eq!(
        wildcard_index
            .get(&UpstreamProtocol::Openai)
            .expect("wildcard protocol entry"),
        &[0]
    );
    assert_eq!(
        route_index
            .get(&UpstreamProtocol::Openai)
            .and_then(|models| models.get("gpt-4.1"))
            .expect("strict model entry"),
        &[1]
    );
}

#[tokio::test]
async fn refreshed_credential_invalidation_is_targeted_and_stales_routing_cache() {
    let selector = Selector::with_cache_ttl(Duration::from_secs(30));
    {
        let mut cache = selector.routing_cache.write().await;
        let routing = RoutingCache {
            loaded_at: Some(Instant::now()),
            ..Default::default()
        };
        *cache = Arc::new(routing);
    }
    selector.credential_runtime_secrets.insert(
        1,
        CachedRuntimeSecret {
            ciphertext: "cipher-1".to_string(),
            secret: "secret-1".to_string(),
            account_id: Some("acct-1".to_string()),
        },
    );
    selector.credential_runtime_secrets.insert(
        2,
        CachedRuntimeSecret {
            ciphertext: "cipher-2".to_string(),
            secret: "secret-2".to_string(),
            account_id: Some("acct-2".to_string()),
        },
    );

    selector.invalidate_refreshed_credential(1).await;

    assert!(selector
        .credential_runtime_secrets
        .get(1, "cipher-1")
        .is_none());
    assert_eq!(
        selector
            .credential_runtime_secrets
            .get(2, "cipher-2")
            .expect("untouched credential")
            .secret,
        "secret-2"
    );
    assert!(selector.routing_cache.read().await.loaded_at.is_none());
}

#[tokio::test]
async fn selector_reports_affinity_miss_and_local_hit() {
    let selector = Selector::with_cache_ttl(Duration::from_secs(30));
    let channel = candidate("primary", 1, 1, vec!["gpt-5.6"]);
    let (route_index, wildcard_index) = build_route_indexes(std::slice::from_ref(&channel));
    {
        let mut cache = selector.routing_cache.write().await;
        *cache = Arc::new(RoutingCache {
            loaded_at: Some(Instant::now()),
            channels: vec![channel],
            keys: HashMap::from([(
                1,
                vec![KeyCandidate {
                    id: 101,
                    channel_id: 1,
                    credential_id: None,
                    secret_ciphertext: "secret".to_string(),
                    cooldown_until: None,
                    plan_type: None,
                    plan_models: Vec::new(),
                }],
            )]),
            model_blocks: HashMap::new(),
            route_index,
            wildcard_index,
        });
    }
    let pool = PgPool::connect_lazy("postgres://neogate:neogate@localhost/neogate").unwrap();
    let secrets = SecretStore::new("test-key", 10);
    let affinity_cache =
        super::super::affinity::ChannelAffinityCache::new(true, Duration::from_secs(60), 10);
    let value: serde_json::Value =
        serde_json::from_str(r#"{"prompt_cache_key":"session-1"}"#).unwrap();
    let affinity_key =
        super::super::affinity::openai_responses_affinity_key_from_value("gpt-5.6", &value)
            .unwrap();

    let first = selector
        .select_with_affinity(
            &pool,
            &secrets,
            &affinity_cache,
            UpstreamProtocol::Openai,
            "gpt-5.6",
            SelectionConstraints {
                affinity_key: Some(&affinity_key),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        first.affinity.as_ref().map(|value| value.status),
        Some(ChannelAffinityStatus::Miss)
    );

    affinity_cache
        .insert(affinity_key.clone(), (&first).into())
        .await;
    let second = selector
        .select_with_affinity(
            &pool,
            &secrets,
            &affinity_cache,
            UpstreamProtocol::Openai,
            "gpt-5.6",
            SelectionConstraints {
                affinity_key: Some(&affinity_key),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        second.affinity.as_ref().map(|value| value.status),
        Some(ChannelAffinityStatus::Local)
    );
    assert_eq!(second.channel_id, first.channel_id);
    assert_eq!(second.channel_endpoint_id, first.channel_endpoint_id);
}

#[test]
fn choose_channel_uses_highest_priority() {
    let low = candidate("low", 0, 100, vec![]);
    let high = candidate("high", 2, 1, vec![]);
    let selected = choose_channel_by_slot(std::slice::from_ref(&high), 0).unwrap();
    assert_eq!(selected.name, "high");
    assert_eq!(choose_channel(&[low, high]).unwrap().priority, 2);
}

#[test]
fn choose_channel_by_slot_respects_weight_ranges() {
    let a = candidate("a", 1, 2, vec![]);
    let b = candidate("b", 1, 3, vec![]);
    assert_eq!(
        choose_channel_by_slot(&[a.clone(), b.clone()], 0)
            .unwrap()
            .name,
        "a"
    );
    assert_eq!(
        choose_channel_by_slot(&[a.clone(), b.clone()], 1)
            .unwrap()
            .name,
        "a"
    );
    assert_eq!(choose_channel_by_slot(&[a, b], 2).unwrap().name, "b");
}

#[test]
fn choose_channel_skips_excluded_endpoint_ids() {
    let mut excluded = candidate("excluded", 1, 1, vec!["gpt-5.4"]);
    excluded.id = 1;
    excluded.endpoint_id = 10;
    let mut alternate = candidate("alternate", 1, 1, vec!["gpt-5.4"]);
    alternate.id = 2;
    alternate.endpoint_id = 20;
    let (route_index, wildcard_index) = build_route_indexes(&[excluded.clone(), alternate.clone()]);
    let cache = RoutingCache {
        loaded_at: None,
        channels: vec![excluded, alternate],
        keys: HashMap::from([
            (
                1,
                vec![KeyCandidate {
                    id: 101,
                    channel_id: 1,
                    credential_id: None,
                    secret_ciphertext: "excluded-key".to_string(),
                    cooldown_until: None,
                    plan_type: None,
                    plan_models: Vec::new(),
                }],
            ),
            (
                2,
                vec![KeyCandidate {
                    id: 201,
                    channel_id: 2,
                    credential_id: None,
                    secret_ciphertext: "alternate-key".to_string(),
                    cooldown_until: None,
                    plan_type: None,
                    plan_models: Vec::new(),
                }],
            ),
        ]),
        model_blocks: HashMap::new(),
        route_index,
        wildcard_index,
    };

    let selected = choose_channel_for_request(
        &cache,
        UpstreamProtocol::Openai,
        "gpt-5.4",
        Utc::now(),
        &empty_block_lookup(),
        &[],
        Some(&[10]),
    )
    .expect("alternate channel");

    assert_eq!(selected.endpoint_id, 20);
}

#[test]
fn choose_channel_returns_none_when_all_candidates_are_excluded() {
    let channel = candidate("excluded", 1, 1, vec!["gpt-5.4"]);
    let (route_index, wildcard_index) = build_route_indexes(std::slice::from_ref(&channel));
    let cache = RoutingCache {
        loaded_at: None,
        channels: vec![channel],
        keys: HashMap::from([(
            1,
            vec![KeyCandidate {
                id: 101,
                channel_id: 1,
                credential_id: None,
                secret_ciphertext: "key".to_string(),
                cooldown_until: None,
                plan_type: None,
                plan_models: Vec::new(),
            }],
        )]),
        model_blocks: HashMap::new(),
        route_index,
        wildcard_index,
    };

    assert!(choose_channel_for_request(
        &cache,
        UpstreamProtocol::Openai,
        "gpt-5.4",
        Utc::now(),
        &empty_block_lookup(),
        &[],
        Some(&[10]),
    )
    .is_none());
}

#[test]
fn matching_channel_count_counts_model_and_wildcard_channels() {
    let mut exact = candidate("exact", 0, 1, vec!["gpt-5.5"]);
    exact.id = 1;
    let mut wildcard = candidate("wildcard", 0, 1, vec![]);
    wildcard.id = 2;
    let mut other_model = candidate("other", 0, 1, vec!["gpt-4.1"]);
    other_model.id = 3;
    let mut other_protocol = candidate("anthropic", 0, 1, vec!["gpt-5.5"]);
    other_protocol.id = 4;
    other_protocol.protocol = UpstreamProtocol::Anthropic;
    let cache = RoutingCache {
        loaded_at: None,
        channels: vec![exact, wildcard, other_model, other_protocol],
        keys: HashMap::new(),
        model_blocks: HashMap::new(),
        route_index: HashMap::new(),
        wildcard_index: HashMap::new(),
    };

    assert_eq!(
        matching_channel_count(&cache, UpstreamProtocol::Openai, "gpt-5.5"),
        2
    );
    assert_eq!(
        matching_channel_count(&cache, UpstreamProtocol::Openai, "gpt-4.1"),
        2
    );
    assert_eq!(
        matching_channel_count(&cache, UpstreamProtocol::Anthropic, "gpt-5.5"),
        1
    );
}

#[test]
fn unavailable_message_reports_wrong_protocol_match() {
    let mut channel = candidate("deepseek", 0, 1, vec!["claude-sonnet-4-5"]);
    channel.protocol = UpstreamProtocol::Openai;
    let cache = RoutingCache {
        loaded_at: None,
        channels: vec![channel],
        keys: HashMap::new(),
        model_blocks: HashMap::new(),
        route_index: HashMap::new(),
        wildcard_index: HashMap::new(),
    };

    let message = unavailable_channel_message(
        &cache,
        UpstreamProtocol::Anthropic,
        "claude-sonnet-4-5",
        Utc::now(),
        &empty_block_lookup(),
    );

    assert!(message.contains("matching channel(s) use another protocol"));
    assert!(message.contains("deepseek (openai)"));
}

#[test]
fn unavailable_message_reports_model_mismatch() {
    let mut channel = candidate("anthropic", 0, 1, vec!["claude-3-5-sonnet-latest"]);
    channel.protocol = UpstreamProtocol::Anthropic;
    let cache = RoutingCache {
        loaded_at: None,
        channels: vec![channel],
        keys: HashMap::new(),
        model_blocks: HashMap::new(),
        route_index: HashMap::new(),
        wildcard_index: HashMap::new(),
    };

    let message = unavailable_channel_message(
        &cache,
        UpstreamProtocol::Anthropic,
        "claude-sonnet-4-5",
        Utc::now(),
        &empty_block_lookup(),
    );

    assert!(message.contains("configured anthropic channels do not include this model"));
}

#[tokio::test]
async fn polling_key_selection_cycles() {
    let channel = candidate("poll", 0, 1, vec![]);
    let keys = vec![
        KeyCandidate {
            id: 1,
            channel_id: channel.id,
            credential_id: None,
            secret_ciphertext: "a".to_string(),
            cooldown_until: None,
            plan_type: None,
            plan_models: Vec::new(),
        },
        KeyCandidate {
            id: 2,
            channel_id: channel.id,
            credential_id: None,
            secret_ciphertext: "b".to_string(),
            cooldown_until: None,
            plan_type: None,
            plan_models: Vec::new(),
        },
    ];

    assert_eq!(
        choose_key(
            &channel,
            &keys,
            "gpt-4.1",
            Utc::now(),
            &empty_block_lookup(),
            &[],
        )
        .unwrap()
        .secret_ciphertext,
        "a"
    );
    assert_eq!(
        choose_key(
            &channel,
            &keys,
            "gpt-4.1",
            Utc::now(),
            &empty_block_lookup(),
            &[],
        )
        .unwrap()
        .secret_ciphertext,
        "b"
    );
    assert_eq!(
        choose_key(
            &channel,
            &keys,
            "gpt-4.1",
            Utc::now(),
            &empty_block_lookup(),
            &[],
        )
        .unwrap()
        .secret_ciphertext,
        "a"
    );
}

#[tokio::test]
async fn random_key_selection_uses_available_keys_only() {
    let mut channel = candidate("random", 0, 1, vec![]);
    channel.key_selection_mode = KeySelectionMode::Random;
    let keys = vec![KeyCandidate {
        id: 1,
        channel_id: channel.id,
        credential_id: None,
        secret_ciphertext: "only-enabled".to_string(),
        cooldown_until: None,
        plan_type: None,
        plan_models: Vec::new(),
    }];

    assert_eq!(
        choose_key(
            &channel,
            &keys,
            "gpt-4.1",
            Utc::now(),
            &empty_block_lookup(),
            &[],
        )
        .unwrap()
        .secret_ciphertext,
        "only-enabled"
    );
}

#[tokio::test]
async fn key_selection_skips_attempted_upstream_identity() {
    let channel = candidate("retry", 0, 1, vec![]);
    let keys = vec![
        KeyCandidate {
            id: 1,
            channel_id: channel.id,
            credential_id: None,
            secret_ciphertext: "attempted".to_string(),
            cooldown_until: None,
            plan_type: None,
            plan_models: Vec::new(),
        },
        KeyCandidate {
            id: 2,
            channel_id: channel.id,
            credential_id: None,
            secret_ciphertext: "next".to_string(),
            cooldown_until: None,
            plan_type: None,
            plan_models: Vec::new(),
        },
    ];
    let attempted = vec![AttemptedUpstream {
        channel_id: channel.id,
        channel_endpoint_id: channel.endpoint_id,
        channel_key_id: Some(1),
        credential_id: None,
    }];

    assert_eq!(
        choose_key(
            &channel,
            &keys,
            "gpt-4.1",
            Utc::now(),
            &empty_block_lookup(),
            &attempted,
        )
        .unwrap()
        .secret_ciphertext,
        "next"
    );
}

#[tokio::test]
async fn key_selection_skips_model_blocked_credential() {
    let mut channel = candidate("oauth", 0, 1, vec!["gpt-5.4"]);
    channel.protocol = UpstreamProtocol::OpenAiOauth;
    channel.use_credentials = true;
    let keys = vec![
        KeyCandidate {
            id: 1,
            channel_id: channel.id,
            credential_id: Some(1),
            secret_ciphertext: "blocked".to_string(),
            cooldown_until: None,
            plan_type: Some("free".to_string()),
            plan_models: vec![PlanModel {
                protocol: UpstreamProtocol::OpenAiOauth,
                model: "gpt-5.4".to_string(),
            }],
        },
        KeyCandidate {
            id: 2,
            channel_id: channel.id,
            credential_id: Some(2),
            secret_ciphertext: "available".to_string(),
            cooldown_until: None,
            plan_type: Some("free".to_string()),
            plan_models: vec![PlanModel {
                protocol: UpstreamProtocol::OpenAiOauth,
                model: "gpt-5.4".to_string(),
            }],
        },
    ];
    let now = Utc::now();
    let mut model_blocks = HashMap::new();
    model_blocks.insert(
        ModelBlockKey {
            protocol: UpstreamProtocol::OpenAiOauth,
            endpoint_id: channel.endpoint_id,
            channel_key_id: None,
            credential_id: Some(1),
            model: "gpt-5.4".to_string(),
        },
        now + chrono::Duration::hours(1),
    );
    let local_blocks = ModelBlockCache::default();
    let model_blocks = ModelBlockLookup::new(&model_blocks, &local_blocks);

    assert_eq!(
        choose_key(&channel, &keys, "gpt-5.4", now, &model_blocks, &[])
            .unwrap()
            .secret_ciphertext,
        "available"
    );
}

#[tokio::test]
async fn key_selection_skips_model_outside_credential_plan() {
    let mut channel = candidate("oauth", 0, 1, vec!["gpt-5.4"]);
    channel.protocol = UpstreamProtocol::OpenAiOauth;
    channel.use_credentials = true;
    let keys = vec![
        KeyCandidate {
            id: 1,
            channel_id: channel.id,
            credential_id: Some(1),
            secret_ciphertext: "wrong-plan".to_string(),
            cooldown_until: None,
            plan_type: Some("free".to_string()),
            plan_models: vec![PlanModel {
                protocol: UpstreamProtocol::OpenAiOauth,
                model: "gpt-5.2".to_string(),
            }],
        },
        KeyCandidate {
            id: 2,
            channel_id: channel.id,
            credential_id: Some(2),
            secret_ciphertext: "right-plan".to_string(),
            cooldown_until: None,
            plan_type: Some("plus".to_string()),
            plan_models: vec![PlanModel {
                protocol: UpstreamProtocol::OpenAiOauth,
                model: "gpt-5.4".to_string(),
            }],
        },
    ];

    assert_eq!(
        choose_key(
            &channel,
            &keys,
            "gpt-5.4",
            Utc::now(),
            &empty_block_lookup(),
            &[],
        )
        .unwrap()
        .secret_ciphertext,
        "right-plan"
    );
}

#[test]
fn affinity_target_preserves_selected_upstream_identity() {
    let upstream = SelectedUpstream {
        channel_id: 10,
        channel_endpoint_id: 20,
        channel_key_id: Some(30),
        credential_id: None,
        provider: "openai".to_string(),
        channel_name: "primary".to_string(),
        base_url: "https://example.com".to_string(),
        responses_chat_fallback: false,
        secret: "secret".to_string(),
        account_id: None,
        affinity: None,
        adapter_hint: None,
    };

    let target = UpstreamAffinityTarget::from(&upstream);

    assert_eq!(target.channel_id, 10);
    assert_eq!(target.channel_endpoint_id, 20);
    assert_eq!(target.channel_key_id, Some(30));
    assert_eq!(target.credential_id, None);
}
