use std::{
    fmt::{self, Write as _},
    sync::Arc,
    time::Instant,
};

use axum::{
    body::{to_bytes, Body},
    extract::{DefaultBodyLimit, State},
    http::{header, HeaderName, HeaderValue, Method, Request, StatusCode},
    middleware::{self, Next},
    response::Response,
    Router,
};
use chrono::{SecondsFormat, Utc};
use reqwest::Client;
use serde_json::Value;
use tokio::sync::watch;
use tower_http::cors::{Any, CorsLayer};
use tracing::{
    field::{Field, Visit},
    Event, Level, Subscriber,
};
use tracing_subscriber::{
    fmt::{format::Writer, FmtContext, FormatEvent, FormatFields},
    layer::SubscriberExt,
    registry::LookupSpan,
    util::SubscriberInitExt,
};

use crate::{
    admin, auth,
    billing::{outbox::BillingOutbox, Billing},
    cache,
    config::{Config, RuntimeProbe, DEFAULT_RELAY_BODY_LIMIT_BYTES},
    db::Db,
    email::EmailService,
    health::{self, RuntimeHealth},
    id::DbId,
    payment, policy,
    relay::{self, selector::Selector},
    secrets::SecretStore,
    setup::{bootstrap, install},
    task,
    usage::{ActivityRecorder, UsageDailyRecorder, UsageRecorder},
    user,
};

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub usage: UsageRecorder,
    pub usage_daily: UsageDailyRecorder,
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
    pub auth_rate_limiter: auth::AuthRateLimiter,
    pub(crate) image_sync_limiter: relay::ImageSyncLimiter,
    pub service_policy_cache: policy::ServicePolicyCache,
    pub cache_invalidator: cache::CacheInvalidator,
    pub runtime_restart_tx: watch::Sender<bool>,
}

pub async fn run() -> anyhow::Result<()> {
    load_dotenv();
    init_tracing();

    let probe = RuntimeProbe::from_env()?;
    if !probe.full_config_ready() {
        let (bootstrap_restart_tx, bootstrap_restart_rx) = watch::channel(false);
        let app = bootstrap::router(bootstrap_restart_tx)
            .merge(install::bootstrap_router())
            .layer(DefaultBodyLimit::max(DEFAULT_RELAY_BODY_LIMIT_BYTES))
            .layer(cors_layer_from_origins(&["*".to_string()])?)
            .layer(middleware::from_fn(log_bootstrap_http_request));
        let listener = tokio::net::TcpListener::bind(&probe.bind_addr).await?;
        tracing::info!(
            "neogate bootstrap listener running on {} because runtime configuration is incomplete",
            probe.bind_addr
        );
        axum::serve(listener, app)
            .with_graceful_shutdown(runtime_shutdown_signal(bootstrap_restart_rx))
            .await?;
        return Ok(());
    }

    let config = Config::from_env()?;
    let (runtime_restart_tx, runtime_restart_rx) = watch::channel(false);
    let state = build_state(config.clone(), runtime_restart_tx).await?;

    if !config.process_role.runs_api() {
        tracing::info!(
            "neogate running in {} role without HTTP listener",
            config.process_role.as_str()
        );
        shutdown_signal().await;
        return Ok(());
    }

    let app = router(state)
        .layer(DefaultBodyLimit::max(config.relay.body_limit_bytes))
        .layer(cors_layer(&config)?)
        .layer(middleware::from_fn_with_state(
            config.admin_token_secret.clone(),
            log_http_request,
        ));

    let listener = tokio::net::TcpListener::bind(&config.bind_addr).await?;
    tracing::info!("neogate listening on {}", config.bind_addr);
    axum::serve(listener, app)
        .with_graceful_shutdown(runtime_shutdown_signal(runtime_restart_rx))
        .await?;
    Ok(())
}

fn init_tracing() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer().event_format(NeogateLogFormat))
        .init();
}

const ERROR_LOG_BODY_LIMIT_BYTES: usize = 64 * 1024;

async fn log_bootstrap_http_request(request: Request<Body>, next: Next) -> Response {
    log_http_response(
        request,
        next,
        auth::RequestAuthLogContext::new("none", None),
    )
    .await
}

async fn log_http_request(
    State(admin_token_secret): State<String>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    let (auth, user_id) = request_auth_context(&request, &admin_token_secret);
    let auth_context = auth::RequestAuthLogContext::new(auth, user_id);
    request.extensions_mut().insert(auth_context.clone());
    log_http_response(request, next, auth_context).await
}

async fn log_http_response(
    request: Request<Body>,
    next: Next,
    auth_context: auth::RequestAuthLogContext,
) -> Response {
    let started = Instant::now();
    let request_line = format!("{} {}", request.method(), request.uri().path());

    let response = next.run(request).await;
    let status = response.status();
    let (response, error_code, error_message) = response_error_for_log(response).await;
    let elapsed_ms = started.elapsed().as_millis();
    let (auth, subject_id) = auth_context.snapshot();
    log_http_request_event(
        &request_line,
        status,
        elapsed_ms,
        auth,
        subject_id,
        error_code,
        error_message,
    );
    response
}

fn log_http_request_event(
    request_line: &str,
    status: StatusCode,
    elapsed_ms: u128,
    auth: &'static str,
    subject_id: Option<DbId>,
    error_code: Option<String>,
    error_message: Option<String>,
) {
    match (auth, subject_id, error_message) {
        ("admin", Some(admin_id), Some(error_message)) => tracing::info!(
            request = %request_line,
            status = %status.as_u16(),
            elapsed_ms = %elapsed_ms,
            admin_id = %admin_id,
            error_code = %error_code.unwrap_or_else(|| "-".to_string()),
            error_message = %error_message,
            "http request"
        ),
        ("admin", Some(admin_id), None) => tracing::info!(
            request = %request_line,
            status = %status.as_u16(),
            elapsed_ms = %elapsed_ms,
            admin_id = %admin_id,
            "http request"
        ),
        ("user" | "token", Some(user_id), Some(error_message)) => tracing::info!(
            request = %request_line,
            status = %status.as_u16(),
            elapsed_ms = %elapsed_ms,
            user_id = %user_id,
            error_code = %error_code.unwrap_or_else(|| "-".to_string()),
            error_message = %error_message,
            "http request"
        ),
        ("user" | "token", Some(user_id), None) => tracing::info!(
            request = %request_line,
            status = %status.as_u16(),
            elapsed_ms = %elapsed_ms,
            user_id = %user_id,
            "http request"
        ),
        (_, _, Some(error_message)) => tracing::info!(
            request = %request_line,
            status = %status.as_u16(),
            elapsed_ms = %elapsed_ms,
            error_code = %error_code.unwrap_or_else(|| "-".to_string()),
            error_message = %error_message,
            "http request"
        ),
        _ => tracing::info!(
            request = %request_line,
            status = %status.as_u16(),
            elapsed_ms = %elapsed_ms,
            "http request"
        ),
    }
}

fn request_auth_context(
    request: &Request<Body>,
    admin_token_secret: &str,
) -> (&'static str, Option<DbId>) {
    let Some(token) = auth::bearer(request.headers()) else {
        return ("none", None);
    };
    if let Some(admin_id) = auth::validate_admin_session_token(token, admin_token_secret) {
        return ("admin", Some(admin_id));
    }
    if let Some(user_id) = auth::validate_user_session_token(token, admin_token_secret) {
        return ("user", Some(user_id));
    }
    ("none", None)
}

async fn response_error_for_log(response: Response) -> (Response, Option<String>, Option<String>) {
    if response.status().is_success() {
        return (response, None, None);
    }

    let status = response.status();
    if response
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok())
        .is_some_and(|length| length > ERROR_LOG_BODY_LIMIT_BYTES)
    {
        return (
            response,
            None,
            Some(format!(
                "{} response body omitted from log because content-length exceeds {} bytes",
                status.canonical_reason().unwrap_or("error"),
                ERROR_LOG_BODY_LIMIT_BYTES
            )),
        );
    }

    let (parts, body) = response.into_parts();
    let bytes = match to_bytes(body, ERROR_LOG_BODY_LIMIT_BYTES).await {
        Ok(bytes) => bytes,
        Err(err) => {
            return (
                Response::from_parts(parts, Body::empty()),
                None,
                Some(format!("failed to read error response body: {err}")),
            );
        }
    };
    let error = parse_error_body_for_log(status, &bytes);
    (
        Response::from_parts(parts, Body::from(bytes)),
        error.0,
        error.1,
    )
}

fn parse_error_body_for_log(status: StatusCode, bytes: &[u8]) -> (Option<String>, Option<String>) {
    if bytes.is_empty() {
        return (
            None,
            Some(status.canonical_reason().unwrap_or("error").to_string()),
        );
    }

    if let Ok(value) = serde_json::from_slice::<Value>(bytes) {
        if let Some(error) = value.get("error") {
            let code = error
                .get("code")
                .and_then(Value::as_str)
                .or_else(|| error.get("type").and_then(Value::as_str))
                .map(str::to_string);
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .map(truncate_log_value);
            if code.is_some() || message.is_some() {
                return (code, message);
            }
        }
    }

    let message = std::str::from_utf8(bytes)
        .ok()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(truncate_log_value)
        .unwrap_or_else(|| status.canonical_reason().unwrap_or("error").to_string());
    (None, Some(message))
}

fn truncate_log_value(value: &str) -> String {
    const MAX_CHARS: usize = 240;
    let mut result = String::new();
    for (index, ch) in value.chars().enumerate() {
        if index >= MAX_CHARS {
            result.push('…');
            break;
        }
        result.push(ch);
    }
    result
}

struct NeogateLogFormat;

impl<S, N> FormatEvent<S, N> for NeogateLogFormat
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        _ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let meta = event.metadata();
        let mut fields = LogFields::default();
        event.record(&mut fields);

        write_gray(
            &mut writer,
            Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true),
        )?;
        write!(writer, " ")?;
        write_level(&mut writer, *meta.level())?;
        write!(writer, " ")?;
        write_gray(&mut writer, format_args!("{}:", meta.target()))?;

        if let Some(message) = fields.message {
            write!(writer, " {message}")?;
        }

        if !fields.fields.is_empty() {
            write!(writer, " ")?;
            write_gray(&mut writer, "|")?;
            write!(writer, " {}", fields.fields)?;
        }

        writeln!(writer)
    }
}

fn write_gray(writer: &mut Writer<'_>, value: impl fmt::Display) -> fmt::Result {
    if writer.has_ansi_escapes() {
        write!(writer, "\x1b[90m{value}\x1b[0m")
    } else {
        write!(writer, "{value}")
    }
}

fn write_level(writer: &mut Writer<'_>, level: Level) -> fmt::Result {
    if !writer.has_ansi_escapes() {
        return write!(writer, "{level:>5}");
    }

    let color = match level {
        Level::ERROR => "\x1b[31;1m",
        Level::WARN => "\x1b[33;1m",
        Level::INFO => "\x1b[32m",
        Level::DEBUG => "\x1b[34m",
        Level::TRACE => "\x1b[35m",
    };
    write!(writer, "{color}{level:>5}\x1b[0m")
}

#[derive(Default)]
struct LogFields {
    message: Option<String>,
    fields: String,
}

impl LogFields {
    fn record_value(&mut self, field: &Field, value: String) {
        if field.name() == "message" {
            self.message = Some(value);
        } else {
            if !self.fields.is_empty() {
                self.fields.push(' ');
            }
            let _ = write!(self.fields, "{}={}", field.name(), value);
        }
    }
}

impl Visit for LogFields {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.record_value(field, format!("{value:?}"));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.record_value(field, value.to_string());
    }
}

async fn build_state(
    config: Config,
    runtime_restart_tx: watch::Sender<bool>,
) -> anyhow::Result<Arc<AppState>> {
    let db = Db::connect(&config).await?;
    sqlx::migrate!("./migrations").run(&db.pool).await?;
    let secrets = SecretStore::new(&config.upstream_secret_key, config.cache.secret_max_entries);
    let email = EmailService::new(db.clone(), secrets.clone());

    let http = Client::builder()
        .read_timeout(config.http.upstream_timeout)
        .connect_timeout(config.http.upstream_connect_timeout)
        .pool_max_idle_per_host(config.http.pool_max_idle_per_host)
        .pool_idle_timeout(config.http.pool_idle_timeout)
        .tcp_nodelay(true)
        .build()?;
    let redis = if config.runtime_mode.is_distributed() {
        Some(redis::Client::open(
            config.redis_url.as_deref().expect("validated redis url"),
        )?)
    } else {
        None
    };
    let selector = Selector::with_cache_ttl(config.cache.routing_ttl);
    let billing = if config.runtime_mode.is_distributed() {
        Billing::new_redis(
            config.redis_url.as_deref().expect("validated redis url"),
            config.redis_key_prefix.clone(),
            config.cache.price_ttl,
            config.cache.price_max_entries,
            config.billing.credit_prefetch_micro_usd,
            config.billing.default_output_tokens,
        )
        .await?
    } else {
        Billing::new_memory(
            config.cache.price_ttl,
            config.cache.price_max_entries,
            config.billing.credit_prefetch_micro_usd,
            config.billing.default_output_tokens,
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
    let usage_daily = UsageDailyRecorder::spawn(
        db.pool.clone(),
        config
            .usage_queue
            .flush_interval
            .saturating_mul(10)
            .max(std::time::Duration::from_secs(5)),
    );
    let activity = ActivityRecorder::spawn(
        db.pool.clone(),
        config
            .usage_queue
            .flush_interval
            .saturating_mul(10)
            .max(std::time::Duration::from_secs(10)),
    );
    let usage = UsageRecorder::spawn(
        db.pool.clone(),
        config.usage_queue.flush_interval,
        config.usage_queue.size,
        activity.clone(),
        usage_daily.clone(),
    );
    let credential_models = relay::CredentialModelRecorder::spawn(
        db.pool.clone(),
        config.usage_queue.flush_interval,
        config.usage_queue.size,
    );
    let billing_outbox = BillingOutbox::spawn(
        db.pool.clone(),
        config.usage_queue.flush_interval,
        config.usage_queue.size,
        activity,
        usage_daily.clone(),
        config.process_role.runs_background(),
    );
    if config.process_role.runs_background() {
        billing.spawn_allocation_recovery(
            db.pool.clone(),
            config.billing.credit_allocation_recovery_interval,
            config.billing.credit_allocation_recovery_after,
        );
    }

    let auth_rate_limiter = if config.runtime_mode.is_distributed() {
        auth::AuthRateLimiter::redis(
            config.redis_url.as_deref().expect("validated redis url"),
            config.redis_key_prefix.clone(),
        )
        .await?
    } else {
        auth::AuthRateLimiter::local()
    };

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
            config.cache.user_auth_ttl,
            config.cache.user_auth_max_entries,
        ),
        auth_rate_limiter,
        image_sync_limiter: relay::ImageSyncLimiter::new(
            config.relay.image_sync_global_limit,
            config.relay.image_sync_key_limit,
        ),
        service_policy_cache: policy::ServicePolicyCache::default(),
        cache_invalidator,
        runtime_restart_tx,
    });
    if config.process_role.runs_api() {
        if let Some(listener) = invalidation_listener {
            listener.spawn(Arc::clone(&state));
        }
    }
    task::worker::spawn(Arc::clone(&state));

    Ok(state)
}

fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .merge(auth::router())
        .merge(admin::router())
        .merge(user::router())
        .merge(health::router())
        .merge(install::router())
        .merge(payment::router())
        .merge(policy::router())
        .merge(admin::public_router())
        .merge(relay::router())
        .with_state(state)
}

fn cors_layer(config: &Config) -> anyhow::Result<CorsLayer> {
    cors_layer_from_origins(&config.cors_allowed_origins)
}

fn cors_layer_from_origins(cors_allowed_origins: &[String]) -> anyhow::Result<CorsLayer> {
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

    if cors_allowed_origins.iter().any(|origin| origin == "*") {
        return Ok(layer.allow_origin(Any));
    }

    let origins = cors_allowed_origins
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

async fn runtime_shutdown_signal(mut restart_rx: watch::Receiver<bool>) {
    let restart_requested = async {
        loop {
            if *restart_rx.borrow() {
                break;
            }
            if restart_rx.changed().await.is_err() {
                break;
            }
        }
    };

    tokio::select! {
        _ = shutdown_signal() => {},
        _ = restart_requested => {
            tracing::info!("runtime configuration saved; gracefully shutting down for restart");
        },
    }
}

fn load_dotenv() {
    if let Ok(path) = std::env::var("NEOGATE_ENV_FILE") {
        dotenvy::from_path(path).ok();
    }
    dotenvy::from_filename(".env").ok();
}

#[cfg(test)]
pub(crate) mod tests {
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

    pub(crate) fn test_state() -> Arc<AppState> {
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://localhost/neogate")
            .unwrap();
        let (runtime_restart_tx, _runtime_restart_rx) = watch::channel(false);
        Arc::new(AppState {
            config: Config {
                database_url: "postgres://localhost/neogate".to_string(),
                bind_addr: "127.0.0.1:0".parse().unwrap(),
                public_base_url: Some("http://localhost:8080".to_string()),
                site_name: "NeoGate".to_string(),
                runtime_mode: config::RuntimeMode::Standalone,
                process_role: config::ProcessRole::All,
                admin_token_secret: "test-admin-token-secret".to_string(),
                admin_session_ttl: Duration::from_secs(3600),
                upstream_secret_key: "test-upstream-secret-key".to_string(),
                http: config::HttpClientConfig {
                    upstream_connect_timeout: Duration::from_secs(10),
                    upstream_timeout: Duration::from_secs(30),
                    pool_max_idle_per_host: 100,
                    pool_idle_timeout: Duration::from_secs(90),
                },
                relay: config::RelayConfig {
                    key_cooldown: Duration::from_secs(60),
                    body_limit_bytes: config::DEFAULT_RELAY_BODY_LIMIT_BYTES,
                    usage_buffer_limit_bytes: config::DEFAULT_RELAY_USAGE_BUFFER_LIMIT_BYTES,
                    credential_upload_limit_bytes: config::DEFAULT_CREDENTIAL_UPLOAD_LIMIT_BYTES,
                    image_sync_global_limit: 8,
                    image_sync_key_limit: 2,
                },
                cache: config::CacheConfig {
                    user_auth_ttl: Duration::from_secs(30),
                    user_auth_max_entries: 1024,
                    routing_ttl: Duration::from_secs(30),
                    price_ttl: Duration::from_secs(30),
                    price_max_entries: 1024,
                    secret_max_entries: 1024,
                },
                redis_url: None,
                redis_key_prefix: "neogate-test".to_string(),
                billing: config::BillingConfig {
                    credit_prefetch_micro_usd: 100_000,
                    credit_allocation_recovery_after: Duration::from_secs(3600),
                    credit_allocation_recovery_interval: Duration::from_secs(60),
                    default_output_tokens: 4096,
                },
                usage_queue: config::UsageQueueConfig {
                    flush_interval: Duration::from_secs(1),
                    size: 1024,
                },
                health: config::HealthConfig {
                    billing_outbox_max_pending: 10_000,
                    billing_outbox_max_age: Duration::from_secs(300),
                },
                task: config::TaskConfig {
                    upstream_poll_interval: Duration::from_secs(30),
                    upstream_poll_batch_size: 100,
                    upstream_retention: Duration::from_secs(2_592_000),
                },
                response_assets: config::ResponseAssetConfig {
                    dir: std::env::temp_dir().join("neogate-test-assets"),
                    retention: Duration::from_secs(604_800),
                },
                db_pool: config::DbPoolConfig {
                    min_connections: 0,
                    max_connections: 10,
                    acquire_timeout: Duration::from_secs(5),
                },
                cors_allowed_origins: vec!["*".to_string()],
            },
            usage: UsageRecorder::disabled(),
            usage_daily: UsageDailyRecorder::disabled(),
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
            auth_rate_limiter: auth::AuthRateLimiter::default(),
            image_sync_limiter: relay::ImageSyncLimiter::new(8, 2),
            service_policy_cache: policy::ServicePolicyCache::default(),
            cache_invalidator: cache::CacheInvalidator::local(),
            runtime_restart_tx,
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
    async fn image_relay_without_user_key_returns_unauthorized() {
        let state = test_state();
        let app = Router::new().merge(relay::router()).with_state(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/images/generations")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"model":"gpt-image-1","prompt":"draw a teapot"}"#,
                    ))
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
    async fn protected_admin_accepts_signed_session_token() {
        let state = test_state();
        let token = auth::issue_admin_token(
            state.config.admin_session_ttl,
            &state.config.admin_token_secret,
            1,
        );
        let app = Router::new()
            .merge(admin::router())
            .route("/protected-admin", get(protected_admin))
            .with_state(state);

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

    #[tokio::test]
    async fn large_error_response_log_keeps_body_unread() {
        let body = "x".repeat(ERROR_LOG_BODY_LIMIT_BYTES + 1);
        let response = Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .header(header::CONTENT_LENGTH, body.len().to_string())
            .body(Body::from(body.clone()))
            .unwrap();

        let (response, code, message) = response_error_for_log(response).await;

        assert!(code.is_none());
        assert!(message
            .as_deref()
            .unwrap_or_default()
            .contains("omitted from log"));
        let bytes = to_bytes(response.into_body(), body.len() + 1)
            .await
            .unwrap();
        assert_eq!(bytes.len(), body.len());
    }
}
