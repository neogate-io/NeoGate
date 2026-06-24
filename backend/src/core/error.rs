use axum::{
    http::{HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpstreamErrorKind {
    Timeout,
    Tls,
    Dns,
    Connect,
    Request,
}

impl UpstreamErrorKind {
    pub(crate) fn from_reqwest(err: &reqwest::Error) -> Self {
        if err.is_timeout() {
            return Self::Timeout;
        }

        let details = format!("{err:?}").to_ascii_lowercase();
        if details.contains("tls") {
            Self::Tls
        } else if details.contains("dns") || details.contains("resolve") {
            Self::Dns
        } else if err.is_connect() {
            Self::Connect
        } else {
            Self::Request
        }
    }

    pub fn status(self) -> StatusCode {
        match self {
            Self::Timeout => StatusCode::GATEWAY_TIMEOUT,
            Self::Tls | Self::Dns | Self::Connect | Self::Request => StatusCode::BAD_GATEWAY,
        }
    }

    pub(crate) fn retryable(self) -> bool {
        true
    }

    pub fn type_code(self) -> &'static str {
        match self {
            Self::Timeout => "upstream_timeout",
            Self::Tls => "upstream_tls_error",
            Self::Dns => "upstream_dns_error",
            Self::Connect => "upstream_connect_error",
            Self::Request => "upstream_request_error",
        }
    }

    pub(crate) fn user_message(self) -> &'static str {
        match self {
            Self::Timeout => {
                "The upstream provider did not respond in time. Please retry later or switch to another channel."
            }
            Self::Tls | Self::Dns | Self::Connect | Self::Request => {
                "The upstream provider is temporarily unavailable. Please retry later or switch to another channel."
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct UpstreamRequestError {
    pub kind: UpstreamErrorKind,
    pub provider: String,
    pub detail: String,
    pub retryable: bool,
}

impl UpstreamRequestError {
    pub fn new(
        kind: UpstreamErrorKind,
        provider: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            provider: provider.into(),
            detail: detail.into(),
            retryable: true,
        }
    }

    pub fn status(&self) -> StatusCode {
        self.kind.status()
    }

    pub(crate) fn from_reqwest(provider: impl Into<String>, err: &reqwest::Error) -> Self {
        Self::new(
            UpstreamErrorKind::from_reqwest(err),
            provider,
            format!("{err:?}"),
        )
    }
}

impl std::fmt::Display for UpstreamRequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} from {}: {}",
            self.kind.type_code(),
            self.provider,
            self.detail
        )
    }
}

impl std::error::Error for UpstreamRequestError {}

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("unauthorized")]
    Unauthorized,
    #[error("forbidden")]
    Forbidden,
    #[error("password change required")]
    PasswordChangeRequired,
    #[error("payment required")]
    PaymentRequired,
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("conflict: {message}")]
    ConflictWithCode {
        code: &'static str,
        message: &'static str,
    },
    #[error("payload too large: {0}")]
    PayloadTooLarge(String),
    #[error("not found")]
    NotFound,
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("bad request: {message}")]
    BadRequestWithCode {
        code: &'static str,
        message: &'static str,
    },
    #[error("rate limited: {0}")]
    RateLimited(String),
    #[error("upstream unavailable: {0}")]
    UpstreamUnavailable(String),
    #[error(transparent)]
    UpstreamRequest(UpstreamRequestError),
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),
    #[error(transparent)]
    Redis(#[from] redis::RedisError),
    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        if let AppError::UpstreamRequest(err) = self {
            return upstream_request_response(err);
        }

        let status = self.status();
        let code = self.code();
        let message = self.user_message(status);
        self.log(status, code);
        error_response(status, code, message)
    }
}

impl AppError {
    pub(crate) fn retryable(&self) -> bool {
        match self {
            AppError::UpstreamRequest(err) => err.retryable,
            AppError::Reqwest(err) => UpstreamErrorKind::from_reqwest(err).retryable(),
            _ => false,
        }
    }

    fn status(&self) -> StatusCode {
        match self {
            AppError::Unauthorized => StatusCode::UNAUTHORIZED,
            AppError::Forbidden => StatusCode::FORBIDDEN,
            AppError::PasswordChangeRequired => StatusCode::FORBIDDEN,
            AppError::PaymentRequired => StatusCode::PAYMENT_REQUIRED,
            AppError::Conflict(_) | AppError::ConflictWithCode { .. } => StatusCode::CONFLICT,
            AppError::PayloadTooLarge(_) => StatusCode::PAYLOAD_TOO_LARGE,
            AppError::NotFound => StatusCode::NOT_FOUND,
            AppError::BadRequest(_) | AppError::BadRequestWithCode { .. } => {
                StatusCode::BAD_REQUEST
            }
            AppError::RateLimited(_) => StatusCode::TOO_MANY_REQUESTS,
            AppError::UpstreamUnavailable(_) => StatusCode::BAD_GATEWAY,
            AppError::UpstreamRequest(err) => err.status(),
            AppError::Sqlx(_)
            | AppError::Io(_)
            | AppError::Json(_)
            | AppError::Reqwest(_)
            | AppError::Redis(_)
            | AppError::Anyhow(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn code(&self) -> &'static str {
        match self {
            AppError::Unauthorized => "unauthorized",
            AppError::Forbidden => "forbidden",
            AppError::PasswordChangeRequired => "password_change_required",
            AppError::PaymentRequired => "payment_required",
            AppError::Conflict(_) => "conflict",
            AppError::ConflictWithCode { code, .. } => code,
            AppError::PayloadTooLarge(_) => "payload_too_large",
            AppError::NotFound => "not_found",
            AppError::BadRequest(_) => "bad_request",
            AppError::BadRequestWithCode { code, .. } => code,
            AppError::RateLimited(_) => "rate_limited",
            AppError::UpstreamUnavailable(_) => "upstream_unavailable",
            AppError::UpstreamRequest(err) => err.kind.type_code(),
            AppError::Sqlx(_)
            | AppError::Io(_)
            | AppError::Json(_)
            | AppError::Reqwest(_)
            | AppError::Redis(_)
            | AppError::Anyhow(_) => "internal_server_error",
        }
    }

    fn user_message(&self, status: StatusCode) -> String {
        if let AppError::UpstreamUnavailable(message) = self {
            return message.clone();
        }

        if status.is_server_error() {
            return "internal server error".to_string();
        }

        match self {
            AppError::Conflict(message)
            | AppError::PayloadTooLarge(message)
            | AppError::BadRequest(message)
            | AppError::RateLimited(message) => message.clone(),
            AppError::ConflictWithCode { message, .. } => (*message).to_string(),
            AppError::BadRequestWithCode { message, .. } => (*message).to_string(),
            _ => self.to_string(),
        }
    }

    fn log(&self, status: StatusCode, code: &'static str) {
        if status.is_server_error() {
            tracing::error!(
                code,
                status = status.as_u16(),
                error = %self,
                "request failed"
            );
        } else if self.should_warn() {
            tracing::warn!(
                code,
                status = status.as_u16(),
                error = %self,
                "request rejected"
            );
        }
    }

    fn should_warn(&self) -> bool {
        matches!(
            self,
            AppError::Conflict(_)
                | AppError::ConflictWithCode { .. }
                | AppError::BadRequest(_)
                | AppError::BadRequestWithCode { .. }
                | AppError::PayloadTooLarge(_)
                | AppError::RateLimited(_)
        )
    }
}

fn error_response(status: StatusCode, code: &'static str, message: String) -> Response {
    let mut response = (
        status,
        Json(json!({
            "error": {
                "code": code,
                "message": message,
            }
        })),
    )
        .into_response();
    response
        .headers_mut()
        .insert("x-neogate-error-code", HeaderValue::from_static(code));
    response
}

fn upstream_request_response(err: UpstreamRequestError) -> Response {
    let status = err.status();
    let mut response = (
        status,
        Json(json!({
            "error": {
                "message": err.kind.user_message(),
                "code": err.kind.type_code(),
                "upstream": err.provider,
                "retryable": err.retryable,
            }
        })),
    )
        .into_response();
    response.headers_mut().insert(
        "x-neogate-error-code",
        HeaderValue::from_static(err.kind.type_code()),
    );
    response.headers_mut().insert(
        "x-neogate-retryable",
        HeaderValue::from_static(if err.retryable { "true" } else { "false" }),
    );
    if let Ok(provider) = HeaderValue::from_str(&err.provider) {
        response
            .headers_mut()
            .insert("x-neogate-upstream-provider", provider);
    }
    response
}

pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use axum::{body::to_bytes, http::StatusCode};

    use super::*;

    #[tokio::test]
    async fn server_errors_return_generic_message() {
        let response =
            AppError::Anyhow(anyhow::anyhow!("database password leaked")).into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["error"]["code"], "internal_server_error");
        assert_eq!(value["error"]["message"], "internal server error");
    }

    #[tokio::test]
    async fn client_errors_keep_actionable_message() {
        let response = AppError::BadRequest("invalid provider".to_string()).into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["error"]["code"], "bad_request");
        assert_eq!(value["error"]["message"], "invalid provider");
    }

    #[tokio::test]
    async fn coded_client_errors_return_structured_payload() {
        let response = AppError::BadRequestWithCode {
            code: "smtp_authentication_failed",
            message: "SMTP authentication failed",
        }
        .into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["error"]["code"], "smtp_authentication_failed");
        assert_eq!(value["error"]["message"], "SMTP authentication failed");
    }

    #[tokio::test]
    async fn invalid_email_errors_return_structured_payload() {
        let response = AppError::BadRequestWithCode {
            code: "invalid_email",
            message: "invalid email",
        }
        .into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["error"]["code"], "invalid_email");
        assert_eq!(value["error"]["message"], "invalid email");
    }

    #[tokio::test]
    async fn coded_conflicts_return_structured_payload() {
        let response = AppError::ConflictWithCode {
            code: "user_email_exists",
            message: "user email already exists",
        }
        .into_response();
        assert_eq!(response.status(), StatusCode::CONFLICT);

        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["error"]["code"], "user_email_exists");
        assert_eq!(value["error"]["message"], "user email already exists");
    }

    #[tokio::test]
    async fn upstream_unavailable_keeps_actionable_message() {
        let response = AppError::UpstreamUnavailable("no available anthropic channel".to_string())
            .into_response();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);

        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["error"]["code"], "upstream_unavailable");
        assert_eq!(value["error"]["message"], "no available anthropic channel");
    }

    #[tokio::test]
    async fn upstream_request_errors_return_retryable_gateway_response() {
        let response = AppError::UpstreamRequest(UpstreamRequestError::new(
            UpstreamErrorKind::Tls,
            "gavinhub",
            "tls handshake eof",
        ))
        .into_response();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        assert_eq!(
            response.headers()["x-neogate-error-code"],
            "upstream_tls_error"
        );
        assert_eq!(response.headers()["x-neogate-retryable"], "true");
        assert_eq!(
            response.headers()["x-neogate-upstream-provider"],
            "gavinhub"
        );

        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["error"]["code"], "upstream_tls_error");
        assert_eq!(value["error"]["upstream"], "gavinhub");
        assert_eq!(value["error"]["retryable"], true);
    }

    #[tokio::test]
    async fn upstream_timeouts_return_gateway_timeout() {
        let response = AppError::UpstreamRequest(UpstreamRequestError::new(
            UpstreamErrorKind::Timeout,
            "gavinhub",
            "response headers timed out",
        ))
        .into_response();
        assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);

        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["error"]["code"], "upstream_timeout");
    }
}
