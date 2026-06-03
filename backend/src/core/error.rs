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
    pub fn status(self) -> StatusCode {
        match self {
            Self::Timeout => StatusCode::GATEWAY_TIMEOUT,
            Self::Tls | Self::Dns | Self::Connect | Self::Request => StatusCode::BAD_GATEWAY,
        }
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

    fn user_message(self) -> &'static str {
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
    #[error("payment required")]
    PaymentRequired,
    #[error("payload too large: {0}")]
    PayloadTooLarge(String),
    #[error("not found")]
    NotFound,
    #[error("bad request: {0}")]
    BadRequest(String),
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
            let status = err.status();
            let mut response = (
                status,
                Json(json!({
                    "error": {
                        "message": err.kind.user_message(),
                        "type": err.kind.type_code(),
                        "upstream": err.provider,
                        "retryable": err.retryable,
                    }
                })),
            )
                .into_response();
            response.headers_mut().insert(
                "x-neogate-error-type",
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
            return response;
        }

        let status = match &self {
            AppError::Unauthorized => StatusCode::UNAUTHORIZED,
            AppError::Forbidden => StatusCode::FORBIDDEN,
            AppError::PaymentRequired => StatusCode::PAYMENT_REQUIRED,
            AppError::PayloadTooLarge(_) => StatusCode::PAYLOAD_TOO_LARGE,
            AppError::NotFound => StatusCode::NOT_FOUND,
            AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
            AppError::UpstreamUnavailable(_) => StatusCode::BAD_GATEWAY,
            AppError::UpstreamRequest(_) => unreachable!("handled above"),
            AppError::Sqlx(_)
            | AppError::Io(_)
            | AppError::Json(_)
            | AppError::Reqwest(_)
            | AppError::Redis(_)
            | AppError::Anyhow(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        if status.is_server_error() {
            tracing::error!(error = %self, error_debug = ?self, "request failed");
        } else if matches!(self, AppError::BadRequest(_) | AppError::PayloadTooLarge(_)) {
            tracing::warn!(error = %self, "request rejected");
        }
        let message = match &self {
            AppError::UpstreamUnavailable(_) => self.to_string(),
            _ if status.is_server_error() => "internal server error".to_string(),
            _ => self.to_string(),
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
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
        assert_eq!(value["error"], "internal server error");
    }

    #[tokio::test]
    async fn client_errors_keep_actionable_message() {
        let response = AppError::BadRequest("invalid provider".to_string()).into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["error"], "bad request: invalid provider");
    }

    #[tokio::test]
    async fn upstream_unavailable_keeps_actionable_message() {
        let response = AppError::UpstreamUnavailable("no available anthropic channel".to_string())
            .into_response();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);

        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            value["error"],
            "upstream unavailable: no available anthropic channel"
        );
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
            response.headers()["x-neogate-error-type"],
            "upstream_tls_error"
        );
        assert_eq!(response.headers()["x-neogate-retryable"], "true");
        assert_eq!(
            response.headers()["x-neogate-upstream-provider"],
            "gavinhub"
        );

        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["error"]["type"], "upstream_tls_error");
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
        assert_eq!(value["error"]["type"], "upstream_timeout");
    }
}
