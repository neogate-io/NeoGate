use axum::http::StatusCode;
use serde_json::{json, Value};

const UPSTREAM_ERROR_BODY_LOG_LIMIT: usize = 1000;

pub(crate) struct UpstreamHttpFailure {
    pub(crate) kind: UpstreamFailureKind,
    pub(crate) summary: String,
    pub(crate) detail: String,
    retryable_override: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UpstreamFailureKind {
    QuotaExhausted,
    RateLimited,
    AuthenticationFailed,
    ModelUnavailable,
    ContextLengthExceeded,
    ContentRejected,
    ServerError,
    BadRequest,
    HttpError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FailureCooldown {
    None,
    Default,
    QuotaExhausted,
}

pub(crate) fn describe_upstream_http_failure(
    status: StatusCode,
    body: &[u8],
) -> UpstreamHttpFailure {
    let detail = upstream_error_detail(body);
    let lowered = detail.to_ascii_lowercase();
    let kind = if is_quota_or_balance_error(&lowered) {
        UpstreamFailureKind::QuotaExhausted
    } else if is_rate_limit_error(status, &lowered) {
        UpstreamFailureKind::RateLimited
    } else if is_auth_error(status, &lowered) {
        UpstreamFailureKind::AuthenticationFailed
    } else if is_model_error(status, &lowered) {
        UpstreamFailureKind::ModelUnavailable
    } else if is_context_length_error(status, &lowered) {
        UpstreamFailureKind::ContextLengthExceeded
    } else if is_safety_error(&lowered) {
        UpstreamFailureKind::ContentRejected
    } else if status.is_server_error() || status.as_u16() == 529 {
        UpstreamFailureKind::ServerError
    } else if status == StatusCode::BAD_REQUEST {
        UpstreamFailureKind::BadRequest
    } else {
        UpstreamFailureKind::HttpError
    };

    UpstreamHttpFailure::new(status, detail, kind)
}

impl UpstreamHttpFailure {
    fn new(status: StatusCode, detail: String, kind: UpstreamFailureKind) -> Self {
        Self {
            kind,
            summary: format!(
                "{}: status {}; {detail}",
                kind.summary_prefix(),
                status.as_u16()
            ),
            detail,
            retryable_override: false,
        }
    }

    pub(crate) fn mark_retryable(&mut self) {
        self.retryable_override = true;
    }

    pub(crate) fn error_code(&self) -> &'static str {
        self.kind.error_code()
    }

    pub(crate) fn relay_status(&self) -> StatusCode {
        self.kind.relay_status()
    }

    pub(crate) fn client_retryable(&self) -> bool {
        self.retryable_override || self.kind.client_retryable()
    }

    pub(crate) fn failoverable(&self) -> bool {
        self.retryable_override || self.kind.failoverable()
    }

    pub(crate) fn cooldown(&self) -> FailureCooldown {
        self.kind.cooldown()
    }

    pub(crate) fn client_payload(
        &self,
        request_path: &str,
        upstream_provider: &str,
        upstream_status: StatusCode,
        message: String,
    ) -> Value {
        if is_anthropic_path(request_path) {
            return json!({
                "type": "error",
                "error": {
                    "type": self.kind.anthropic_error_type(),
                    "message": message
                }
            });
        }

        if self.kind == UpstreamFailureKind::ContextLengthExceeded {
            return json!({
                "error": {
                    "message": message,
                    "type": "invalid_request_error",
                    "param": "input",
                    "code": "context_length_exceeded"
                }
            });
        }

        json!({
            "error": {
                "message": message,
                "code": self.error_code(),
                "upstream": upstream_provider,
                "upstream_status": upstream_status.as_u16(),
                "upstream_error": self.detail,
                "retryable": self.client_retryable(),
            }
        })
    }
}

fn is_anthropic_path(path: &str) -> bool {
    path.starts_with("/v1/messages") || path.starts_with("/anthropic/v1/messages")
}

impl UpstreamFailureKind {
    pub(crate) fn error_code(self) -> &'static str {
        match self {
            Self::QuotaExhausted => "upstream_quota_exhausted",
            Self::RateLimited => "upstream_rate_limited",
            Self::AuthenticationFailed => "upstream_authentication_failed",
            Self::ModelUnavailable => "upstream_model_unavailable",
            Self::ContextLengthExceeded => "upstream_context_length_exceeded",
            Self::ContentRejected => "upstream_content_rejected",
            Self::ServerError => "upstream_server_error",
            Self::BadRequest => "upstream_bad_request",
            Self::HttpError => "upstream_http_error",
        }
    }

    fn summary_prefix(self) -> &'static str {
        match self {
            Self::QuotaExhausted => "upstream quota exhausted",
            Self::RateLimited => "upstream rate limited",
            Self::AuthenticationFailed => "upstream authentication failed",
            Self::ModelUnavailable => "upstream model unavailable",
            Self::ContextLengthExceeded => "upstream context length exceeded",
            Self::ContentRejected => "upstream content rejected",
            Self::ServerError => "upstream server error",
            Self::BadRequest => "upstream bad request",
            Self::HttpError => "upstream http error",
        }
    }

    pub(crate) fn user_message(self) -> &'static str {
        match self {
            Self::QuotaExhausted => "The upstream provider account has insufficient balance or quota. Please switch to another channel or contact the service administrator.",
            Self::RateLimited => "The upstream provider is rate limited. Please retry later or switch to another channel.",
            Self::AuthenticationFailed => "The upstream provider rejected the channel credentials. Please switch to another channel or contact the service administrator.",
            Self::ModelUnavailable => "The upstream provider does not have the requested model available on this channel. Please use another model or switch channels.",
            Self::ContextLengthExceeded => "The request is too large for the upstream model context window. Please shorten the input and retry.",
            Self::ContentRejected => "The upstream provider rejected the request content. Please revise the request and retry.",
            Self::ServerError => "The upstream provider is temporarily unavailable. Please retry later or switch to another channel.",
            Self::BadRequest => "The upstream provider rejected the request format or parameters. Please check the request and retry.",
            Self::HttpError => "The upstream provider rejected the request. Please retry later or switch to another channel.",
        }
    }

    pub(crate) fn relay_status(self) -> StatusCode {
        match self {
            Self::QuotaExhausted => StatusCode::PAYMENT_REQUIRED,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::AuthenticationFailed | Self::ModelUnavailable | Self::HttpError => {
                StatusCode::FAILED_DEPENDENCY
            }
            Self::ContextLengthExceeded | Self::ContentRejected | Self::BadRequest => {
                StatusCode::BAD_REQUEST
            }
            Self::ServerError => StatusCode::BAD_GATEWAY,
        }
    }

    pub(crate) fn client_retryable(self) -> bool {
        matches!(self, Self::RateLimited | Self::ServerError)
    }

    pub(crate) fn failoverable(self) -> bool {
        matches!(
            self,
            Self::QuotaExhausted
                | Self::RateLimited
                | Self::AuthenticationFailed
                | Self::ModelUnavailable
                | Self::ServerError
        )
    }

    pub(crate) fn cooldown(self) -> FailureCooldown {
        match self {
            Self::QuotaExhausted => FailureCooldown::QuotaExhausted,
            Self::RateLimited
            | Self::AuthenticationFailed
            | Self::ModelUnavailable
            | Self::ServerError => FailureCooldown::Default,
            _ => FailureCooldown::None,
        }
    }

    pub(crate) fn anthropic_error_type(self) -> &'static str {
        match self {
            Self::RateLimited => "rate_limit_error",
            Self::QuotaExhausted
            | Self::ContextLengthExceeded
            | Self::ContentRejected
            | Self::BadRequest => "invalid_request_error",
            _ => "api_error",
        }
    }
}

fn upstream_error_detail(body: &[u8]) -> String {
    let raw = String::from_utf8_lossy(body);
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "empty upstream error body".to_string();
    }

    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        if let Some(message) = json_error_field(&value, "message") {
            let mut parts = vec![message];
            if let Some(error_type) = json_error_field(&value, "type") {
                parts.push(format!("type={error_type}"));
            }
            if let Some(code) = json_error_field(&value, "code") {
                parts.push(format!("code={code}"));
            }
            return truncate_for_log(&parts.join("; "));
        }
    }

    truncate_for_log(trimmed)
}

fn json_error_field(value: &Value, field: &str) -> Option<String> {
    value
        .get("error")
        .and_then(|error| error.get(field))
        .or_else(|| value.get(field))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
}

fn is_quota_or_balance_error(lowered: &str) -> bool {
    contains_any(
        lowered,
        &[
            "insufficient_quota",            // OpenAI
            "insufficient quota",            // OpenAI / 通用
            "exceeded your current quota",   // OpenAI
            "quota exceeded",                // AWS Bedrock / 通用
            "quota has been exhausted",      // AWS Bedrock
            "quota exhausted",               // 通用
            "allocationquota",               // Azure OpenAI
            "insufficient balance",          // 国内厂商
            "insufficient credit",           // 国内厂商
            "not enough credits",            // 国内厂商
            "credit balance",                // 国内厂商
            "billing hard limit",            // OpenAI
            // 注意：不使用裸 "billing"，避免将 "invalid billing address"
            // 等与余额无关的错误误判为配额耗尽，触发错误 failover。
            "余额",                          // 国内厂商
            "额度",                          // 国内厂商
            "欠费",                          // 国内厂商
        ],
    )
}

fn is_rate_limit_error(status: StatusCode, lowered: &str) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS
        || contains_any(
            lowered,
            &[
                "rate_limit_exceeded",   // OpenAI
                "rate limit",            // 通用
                "too many requests",     // 通用
                "requests per minute",   // OpenAI
                "tokens per minute",     // OpenAI
                "overloaded_error",      // Anthropic
                "overloaded",            // Anthropic / 国内厂商
                "请求过于频繁",           // 国内厂商
                "限流",                  // 国内厂商
            ],
        )
}

fn is_auth_error(status: StatusCode, lowered: &str) -> bool {
    matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
        || contains_any(
            lowered,
            &[
                "invalid_api_key",       // OpenAI
                "incorrect api key",     // OpenAI
                "invalid api key",       // 通用
                "expired api key",       // 通用
                "authentication",        // 通用
                "unauthorized",          // 通用
                "permission denied",     // 通用
                "forbidden",             // 通用
                "access denied",         // 通用
                "无效的 api key",         // 国内厂商
                "未授权",                 // 国内厂商
                "无权限",                 // 国内厂商
                "未登录",                 // 国内厂商
            ],
        )
}

fn is_model_error(status: StatusCode, lowered: &str) -> bool {
    status == StatusCode::NOT_FOUND || is_model_error_text(lowered)
}

/// 仅按错误文案判定是否属于「模型不可用」类错误。供流式 SSE error 路径复用——
/// 那里没有 HTTP 状态码（上游返回 200，错误藏在 SSE event 里），所以不能走带
/// `status == NOT_FOUND` 判定的 `is_model_error`。
pub(crate) fn is_model_error_text(lowered: &str) -> bool {
    contains_any(
        lowered,
        &[
            "model_not_found",               // OpenAI
            "model not found",               // 通用
            "model_not_available",           // 通用
            "model is not available",        // 通用
            "does not exist",                // OpenAI / 国内厂商
            "doesn't exist",                 // 通用
            "not supported",                 // 通用
            "unsupported model",             // 通用
            "no such model",                 // 通用
            "unknown provider for model",    // 国内适配层
            "no provider for model",         // 国内适配层
            "provider for model",            // 国内适配层
            "模型不存在",                     // 国内厂商
            "模型不可用",                     // 国内厂商
            "不支持的模型",                   // 国内厂商
        ],
    )
}

fn is_context_length_error(status: StatusCode, lowered: &str) -> bool {
    matches!(status, StatusCode::PAYLOAD_TOO_LARGE)
        || contains_any(
            lowered,
            &[
                "context_length_exceeded",   // OpenAI
                "maximum context length",    // OpenAI
                "context window",            // 通用
                "too many tokens",           // 通用
                "input is too long",         // Anthropic / 通用
                "prompt is too long",        // 通用
                "tokens exceeds",            // 国内厂商
                "上下文",                     // 国内厂商
                "输入过长",                   // 国内厂商
                "token 超",                  // 国内厂商
            ],
        )
}

fn is_safety_error(lowered: &str) -> bool {
    contains_any(
        lowered,
        &[
            "content_policy_violation",  // OpenAI
            "content policy",            // OpenAI / 通用
            "safety",                    // 通用
            "moderation",                // OpenAI moderation
            // 注意：不使用裸 "blocked"，该词在网络错误、IP 封禁等场景同样常见，
            // 会误将非内容安全错误归类为 ContentRejected 导致不重试。
            "content blocked",           // Azure Content Safety
            "sensitive content",         // 国内厂商
            "unsafe content",            // 通用
            "内容安全",                  // 国内厂商
            "安全策略",                  // 国内厂商
            "敏感内容",                  // 国内厂商
        ],
    )
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

fn truncate_for_log(value: &str) -> String {
    value.chars().take(UPSTREAM_ERROR_BODY_LOG_LIMIT).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_quota_errors() {
        let failure = describe_upstream_http_failure(
            StatusCode::FORBIDDEN,
            br#"{"error":{"message":"You exceeded your current quota","type":"insufficient_quota","code":"insufficient_quota"}}"#,
        );

        assert_eq!(failure.kind, UpstreamFailureKind::QuotaExhausted);
        assert_eq!(failure.relay_status(), StatusCode::PAYMENT_REQUIRED);
        assert!(!failure.client_retryable());
        assert!(failure.failoverable());
        assert_eq!(failure.cooldown(), FailureCooldown::QuotaExhausted);
        assert!(failure.summary.contains("insufficient_quota"));
    }

    #[test]
    fn classifies_allocation_quota_as_exhausted() {
        let failure = describe_upstream_http_failure(
            StatusCode::TOO_MANY_REQUESTS,
            br#"{"error":{"message":"The account quota has been exhausted.","code":"AllocationQuota"}}"#,
        );

        assert_eq!(failure.kind, UpstreamFailureKind::QuotaExhausted);
        assert!(!failure.client_retryable());
        assert!(failure.failoverable());
        assert_eq!(failure.cooldown(), FailureCooldown::QuotaExhausted);
        assert!(failure.detail.contains("AllocationQuota"));
    }

    #[test]
    fn classifies_chinese_balance_errors() {
        let failure =
            describe_upstream_http_failure(StatusCode::FORBIDDEN, "账户余额不足".as_bytes());

        assert_eq!(failure.kind, UpstreamFailureKind::QuotaExhausted);
        assert!(!failure.client_retryable());
        assert!(failure.failoverable());
    }

    #[test]
    fn classifies_rate_limit_errors_as_retryable() {
        let failure = describe_upstream_http_failure(
            StatusCode::TOO_MANY_REQUESTS,
            br#"{"error":{"message":"Rate limit reached"}}"#,
        );

        assert_eq!(failure.kind, UpstreamFailureKind::RateLimited);
        assert_eq!(failure.relay_status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(failure.client_retryable());
        assert!(failure.failoverable());
    }

    #[test]
    fn classifies_context_length_as_bad_request() {
        let failure = describe_upstream_http_failure(
            StatusCode::BAD_REQUEST,
            br#"{"error":{"message":"maximum context length exceeded"}}"#,
        );

        assert_eq!(failure.kind, UpstreamFailureKind::ContextLengthExceeded);
        assert_eq!(failure.relay_status(), StatusCode::BAD_REQUEST);
        assert!(!failure.client_retryable());
        assert!(!failure.failoverable());
    }

    #[test]
    fn classifies_authentication_errors() {
        let body =
            br#"{"error":{"message":"Incorrect API key provided","type":"invalid_api_key"}}"#;

        let failure = describe_upstream_http_failure(StatusCode::UNAUTHORIZED, body);

        assert_eq!(failure.kind, UpstreamFailureKind::AuthenticationFailed);
        assert_eq!(failure.relay_status(), StatusCode::FAILED_DEPENDENCY);
        assert!(!failure.client_retryable());
        assert!(failure.failoverable());
    }

    #[test]
    fn classifies_jdcloud_not_logged_in_as_auth_error() {
        // JDCloud's JoyAgent returns 406 with a JSON body whose `code` is 401
        // and `msg` is "账号未登录" when the channel secret is invalid or
        // expired. This must be treated as an authentication failure rather
        // than the generic upstream_http_error fallback.
        let body = "{\"code\":401,\"data\":null,\"msg\":\"账号未登录\"}".as_bytes();

        let failure = describe_upstream_http_failure(StatusCode::NOT_ACCEPTABLE, body);

        assert_eq!(failure.kind, UpstreamFailureKind::AuthenticationFailed);
        assert_eq!(failure.relay_status(), StatusCode::FAILED_DEPENDENCY);
        assert!(!failure.client_retryable());
        assert!(failure.failoverable());
        assert!(failure.detail.contains("账号未登录"));
    }

    #[test]
    fn classifies_model_errors() {
        let body =
            br#"{"error":{"message":"The model `gpt-x` does not exist","code":"model_not_found"}}"#;

        let failure = describe_upstream_http_failure(StatusCode::NOT_FOUND, body);

        assert_eq!(failure.kind, UpstreamFailureKind::ModelUnavailable);
        assert_eq!(failure.relay_status(), StatusCode::FAILED_DEPENDENCY);
        assert!(!failure.client_retryable());
        assert!(failure.failoverable());
    }

    #[test]
    fn is_model_error_text_matches_bailian_unsupported_model() {
        // 复现阿里云百炼流式 SSE error 的文案：上游返回 200，错误藏在 event:error 里，
        // is_model_error_text 必须仅凭文案命中。
        assert!(is_model_error_text(
            "invalidparameter unsupported model: 'glm-5.2'."
        ));
    }

    #[test]
    fn is_model_error_text_ignores_generic_parameter_errors() {
        assert!(!is_model_error_text(
            "invalidparameter missing required field: input"
        ));
        assert!(!is_model_error_text("invalid_request_error"));
    }

    #[test]
    fn classifies_provider_model_mapping_errors() {
        let body = br#"{"error":{"message":"unknown provider for model gpt-5.2","type":"server_error","code":"internal_server_error"}}"#;

        let failure = describe_upstream_http_failure(StatusCode::BAD_GATEWAY, body);

        assert_eq!(failure.kind, UpstreamFailureKind::ModelUnavailable);
        assert_eq!(failure.relay_status(), StatusCode::FAILED_DEPENDENCY);
        assert!(!failure.client_retryable());
        assert!(failure.failoverable());
        assert!(failure
            .detail
            .contains("unknown provider for model gpt-5.2"));
    }

    #[test]
    fn classifies_server_errors_as_retryable() {
        let failure =
            describe_upstream_http_failure(StatusCode::INTERNAL_SERVER_ERROR, b"backend failed");

        assert_eq!(failure.kind, UpstreamFailureKind::ServerError);
        assert_eq!(failure.relay_status(), StatusCode::BAD_GATEWAY);
        assert!(failure.client_retryable());
        assert!(failure.failoverable());
    }

    #[test]
    fn retryable_override_preserves_error_identity() {
        let mut failure = describe_upstream_http_failure(
            StatusCode::BAD_REQUEST,
            br#"{"error":{"message":"invalid image option"}}"#,
        );

        failure.mark_retryable();

        assert_eq!(failure.kind, UpstreamFailureKind::BadRequest);
        assert_eq!(failure.error_code(), "upstream_bad_request");
        assert_eq!(failure.relay_status(), StatusCode::BAD_REQUEST);
        assert!(failure.client_retryable());
        assert!(failure.failoverable());
        assert_eq!(failure.cooldown(), FailureCooldown::None);
    }
}
