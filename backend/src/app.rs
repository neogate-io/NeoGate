use std::sync::Arc;

use axum::{
    extract::DefaultBodyLimit,
    http::{header, HeaderName, HeaderValue, Method},
    Router,
};
use reqwest::Client;
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::{
    admin, auth,
    billing::{outbox::BillingOutbox, Billing},
    cache,
    config::Config,
    db::Db,
    email::EmailService,
    health::{self, RuntimeHealth},
    payment,
    relay::{self, selector::Selector},
    secrets::SecretStore,
    user,
};

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub usage: relay::UsageRecorder,
    pub usage_daily: relay::UsageDailyRecorder,
    pub credential_models: relay::CredentialModelRecorder,
    pub billing_outbox: BillingOutbox,
    pub billing: Billing,
    pub db: Db,
    pub runtime_health: RuntimeHealth,
    pub email: EmailService,
    pub http: Client,
    pub secrets: SecretStore,
    pub selector: Selector,
    pub user_auth_cache: auth::UserAuthCache,
    pub cache_invalidator: cache::CacheInvalidator,
}

pub async fn run() -> anyhow::Result<()> {
    load_dotenv();
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = Config::from_env()?;
    let state = build_state(config.clone()).await?;

    if !config.process_role.runs_api() {
        tracing::info!(
            "neogate running in {} role without HTTP listener",
            config.process_role.as_str()
        );
        shutdown_signal().await;
        return Ok(());
    }

    let app = router(state)
        .layer(DefaultBodyLimit::max(config.relay_body_limit_bytes))
        .layer(cors_layer(&config)?)
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(&config.bind_addr).await?;
    tracing::info!("neogate listening on {}", config.bind_addr);
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn build_state(config: Config) -> anyhow::Result<Arc<AppState>> {
    let db = Db::connect(&config).await?;
    sqlx::migrate!("./migrations").run(&db.pool).await?;
    let secrets = SecretStore::new(&config.upstream_secret_key, config.secret_cache_max_entries);
    let email = EmailService::new(db.clone(), secrets.clone());

    let http = Client::builder()
        .read_timeout(config.request_timeout)
        .connect_timeout(config.upstream_connect_timeout)
        .pool_max_idle_per_host(config.http_pool_max_idle_per_host)
        .pool_idle_timeout(config.http_pool_idle_timeout)
        .tcp_nodelay(true)
        .build()?;
    let redis = if config.runtime_mode.is_distributed() {
        Some(redis::Client::open(
            config.redis_url.as_deref().expect("validated redis url"),
        )?)
    } else {
        None
    };
    let selector = Selector::with_cache_ttl(config.routing_cache_ttl);
    let billing = if config.runtime_mode.is_distributed() {
        Billing::new_redis(
            config.redis_url.as_deref().expect("validated redis url"),
            config.redis_key_prefix.clone(),
            config.price_cache_ttl,
            config.price_cache_max_entries,
            config.credit_prefetch_micro_usd,
            config.default_output_tokens,
        )
        .await?
    } else {
        Billing::new_memory(
            config.price_cache_ttl,
            config.price_cache_max_entries,
            config.credit_prefetch_micro_usd,
            config.default_output_tokens,
        )
    };
    let (cache_invalidator, invalidation_listener) = if config.runtime_mode.is_distributed() {
        let (invalidator, listener) = cache::CacheInvalidator::redis(
            config.redis_url.as_deref().expect("validated redis url"),
            &config.redis_key_prefix,
        )
        .await?;
        (invalidator, Some(listener))
    } else {
        (cache::CacheInvalidator::local(), None)
    };
    let usage_daily = relay::UsageDailyRecorder::spawn(
        db.pool.clone(),
        config
            .usage_flush_interval
            .saturating_mul(10)
            .max(std::time::Duration::from_secs(5)),
    );
    let activity = relay::ActivityRecorder::spawn(
        db.pool.clone(),
        config
            .usage_flush_interval
            .saturating_mul(10)
            .max(std::time::Duration::from_secs(10)),
    );
    let usage = relay::UsageRecorder::spawn(
        db.pool.clone(),
        config.usage_flush_interval,
        config.usage_queue_size,
        activity.clone(),
        usage_daily.clone(),
    );
    let credential_models = relay::CredentialModelRecorder::spawn(
        db.pool.clone(),
        config.usage_flush_interval,
        config.usage_queue_size,
    );
    let billing_outbox = BillingOutbox::spawn(
        db.pool.clone(),
        config.usage_flush_interval,
        config.usage_queue_size,
        activity,
        usage_daily.clone(),
        config.process_role.runs_background(),
    );
    if config.process_role.runs_background() {
        billing.spawn_allocation_recovery(
            db.pool.clone(),
            config.credit_allocation_recovery_interval,
            config.credit_allocation_recovery_after,
        );
    }

    let state = Arc::new(AppState {
        config: config.clone(),
        usage,
        usage_daily,
        credential_models,
        billing_outbox,
        billing,
        db,
        runtime_health: RuntimeHealth::new(redis),
        email,
        http,
        secrets,
        selector,
        user_auth_cache: auth::UserAuthCache::new(
            config.user_auth_cache_ttl,
            config.user_auth_cache_max_entries,
        ),
        cache_invalidator,
    });
    if config.process_role.runs_api() {
        if let Some(listener) = invalidation_listener {
            listener.spawn(Arc::clone(&state));
        }
    }

    Ok(state)
}

fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .merge(auth::router())
        .merge(admin::router())
        .merge(user::router())
        .merge(health::router())
        .merge(payment::router())
        .merge(admin::public_router())
        .merge(relay::router())
        .with_state(state)
}

fn cors_layer(config: &Config) -> anyhow::Result<CorsLayer> {
    let allowed_methods = [
        Method::GET,
        Method::POST,
        Method::PATCH,
        Method::DELETE,
        Method::OPTIONS,
    ];
    let allowed_headers = [
        header::AUTHORIZATION,
        header::CONTENT_TYPE,
        HeaderName::from_static("x-api-key"),
    ];

    let layer = CorsLayer::new()
        .allow_methods(allowed_methods)
        .allow_headers(allowed_headers);

    if config
        .cors_allowed_origins
        .iter()
        .any(|origin| origin == "*")
    {
        return Ok(layer.allow_origin(Any));
    }

    let origins = config
        .cors_allowed_origins
        .iter()
        .map(|origin| origin.parse::<HeaderValue>())
        .collect::<Result<Vec<_>, _>>()?;
    Ok(layer.allow_origin(origins))
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(err) = tokio::signal::ctrl_c().await {
            tracing::warn!("failed to listen for ctrl-c shutdown signal: {err}");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(err) => tracing::warn!("failed to listen for terminate shutdown signal: {err}"),
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("shutdown signal received");
}

fn load_dotenv() {
    dotenvy::from_filename(".env").ok();
    dotenvy::from_filename("../.env").ok();
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
        routing::get,
        Router,
    };
    use reqwest::Client;
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;

    use super::*;
    use crate::{
        billing::outbox::BillingOutbox,
        config::{self, Config},
        db::Db,
        email::EmailService,
        health::RuntimeHealth,
        secrets::SecretStore,
    };

    fn test_state() -> Arc<AppState> {
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://localhost/neogate")
            .unwrap();
        Arc::new(AppState {
            config: Config {
                database_url: "postgres://localhost/neogate".to_string(),
                bind_addr: "127.0.0.1:0".parse().unwrap(),
                production: false,
                runtime_mode: config::RuntimeMode::Standalone,
                process_role: config::ProcessRole::All,
                admin_username: "admin".to_string(),
                admin_password: "password".to_string(),
                admin_token_secret: "test-admin-token-secret".to_string(),
                admin_session_ttl: Duration::from_secs(3600),
                upstream_secret_key: "test-upstream-secret-key".to_string(),
                anthropic_version: "2023-06-01".to_string(),
                key_cooldown: Duration::from_secs(60),
                request_timeout: Duration::from_secs(60),
                upstream_connect_timeout: Duration::from_secs(10),
                upstream_response_timeout: Duration::from_secs(30),
                relay_body_limit_bytes: config::DEFAULT_RELAY_BODY_LIMIT_BYTES,
                credential_upload_limit_bytes: config::DEFAULT_CREDENTIAL_UPLOAD_LIMIT_BYTES,
                http_pool_max_idle_per_host: 100,
                http_pool_idle_timeout: Duration::from_secs(90),
                user_auth_cache_ttl: Duration::from_secs(30),
                user_auth_cache_max_entries: 1024,
                routing_cache_ttl: Duration::from_secs(30),
                price_cache_ttl: Duration::from_secs(30),
                price_cache_max_entries: 1024,
                secret_cache_max_entries: 1024,
                redis_url: None,
                redis_key_prefix: "neogate-test".to_string(),
                credit_prefetch_micro_usd: 100_000,
                credit_allocation_recovery_after: Duration::from_secs(3600),
                credit_allocation_recovery_interval: Duration::from_secs(60),
                default_output_tokens: 4096,
                usage_flush_interval: Duration::from_secs(1),
                usage_queue_size: 1024,
                billing_outbox_max_pending: 10_000,
                billing_outbox_max_age: Duration::from_secs(300),
                payment: config::PaymentConfig {
                    enabled_providers: Vec::new(),
                    return_base_url: None,
                    zpay: config::ZpayConfig {
                        api_url: Some("https://zpayz.cn/submit.php".to_string()),
                        merchant_id: None,
                        secret_key: None,
                        default_pay_type: "wxpay".to_string(),
                        site_name: "NeoGate".to_string(),
                    },
                },
                db_pool: config::DbPoolConfig {
                    min_connections: 0,
                    max_connections: 10,
                    acquire_timeout: Duration::from_secs(5),
                },
                cors_allowed_origins: vec!["*".to_string()],
            },
            usage: relay::UsageRecorder::disabled(),
            usage_daily: relay::UsageDailyRecorder::disabled(),
            credential_models: relay::CredentialModelRecorder::disabled(),
            billing_outbox: BillingOutbox::new(pool.clone()),
            billing: Billing::new_memory(Duration::from_secs(30), 1024, 100_000, 4096),
            db: Db { pool },
            runtime_health: RuntimeHealth::new(None),
            email: EmailService::test(),
            http: Client::new(),
            secrets: SecretStore::new("test-upstream-secret-key", 1024),
            selector: Selector::new(),
            user_auth_cache: auth::UserAuthCache::new(Duration::from_secs(30), 1024),
            cache_invalidator: cache::CacheInvalidator::local(),
        })
    }

    async fn protected_admin(_admin: auth::AdminAuth) -> &'static str {
        "ok"
    }

    #[tokio::test]
    async fn relay_without_user_key_returns_unauthorized() {
        let state = test_state();
        let app = Router::new().merge(relay::router()).with_state(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"model":"gpt-4.1","messages":[]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn verify_user_key_without_key_returns_unauthorized() {
        let state = test_state();
        let app = Router::new()
            .merge(admin::public_router())
            .with_state(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/user-key/verify")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn admin_login_issues_session_token_instead_of_config_token() {
        let state = test_state();
        let app = Router::new()
            .merge(auth::router())
            .merge(admin::router())
            .route("/protected-admin", get(protected_admin))
            .with_state(state);

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/login")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"username":"admin","password":"password"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let token = value["token"].as_str().unwrap();

        assert_ne!(token, "admin");
        assert!(token.starts_with("neo_admin_"));

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/protected-admin")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/protected-admin")
                    .header("authorization", "Bearer admin")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
