use axum::http::StatusCode;
use serde_json::Value;

const UPSTREAM_ERROR_BODY_LOG_LIMIT: usize = 1000;

pub(crate) struct UpstreamHttpFailure {
    pub(crate) error_type: &'static str,
    pub(crate) user_message: &'static str,
    pub(crate) summary: String,
    pub(crate) detail: String,
    pub(crate) relay_status: StatusCode,
    pub(crate) retryable: bool,
}

pub(crate) fn describe_upstream_http_failure(
    status: StatusCode,
    body: &[u8],
) -> UpstreamHttpFailure {
    let detail = upstream_error_detail(body);
    let lowered = detail.to_ascii_lowercase();
    if is_quota_or_balance_error(&lowered) {
        return upstream_http_failure(
            status,
            detail,
            "upstream_quota_exhausted",
            "upstream quota exhausted",
            "The upstream provider account has insufficient balance or quota. Please switch to another channel or contact the service administrator.",
            StatusCode::BAD_GATEWAY,
            false,
        );
    }

    if is_rate_limit_error(status, &lowered) {
        return upstream_http_failure(
            status,
            detail,
            "upstream_rate_limited",
            "upstream rate limited",
            "The upstream provider is rate limited. Please retry later or switch to another channel.",
            StatusCode::BAD_GATEWAY,
            true,
        );
    }

    if is_auth_error(status, &lowered) {
        return upstream_http_failure(
            status,
            detail,
            "upstream_authentication_failed",
            "upstream authentication failed",
            "The upstream provider rejected the channel credentials. Please switch to another channel or contact the service administrator.",
            StatusCode::BAD_GATEWAY,
            false,
        );
    }

    if is_model_error(status, &lowered) {
        return upstream_http_failure(
            status,
            detail,
            "upstream_model_unavailable",
            "upstream model unavailable",
            "The upstream provider does not have the requested model available on this channel. Please use another model or switch channels.",
            StatusCode::BAD_GATEWAY,
            false,
        );
    }

    if is_context_length_error(status, &lowered) {
        return upstream_http_failure(
            status,
            detail,
            "upstream_context_length_exceeded",
            "upstream context length exceeded",
            "The request is too large for the upstream model context window. Please shorten the input and retry.",
            StatusCode::BAD_REQUEST,
            false,
        );
    }

    if is_safety_error(&lowered) {
        return upstream_http_failure(
            status,
            detail,
            "upstream_content_rejected",
            "upstream content rejected",
            "The upstream provider rejected the request content. Please revise the request and retry.",
            StatusCode::BAD_REQUEST,
            false,
        );
    }

    if status.is_server_error() || status.as_u16() == 529 {
        return upstream_http_failure(
            status,
            detail,
            "upstream_server_error",
            "upstream server error",
            "The upstream provider is temporarily unavailable. Please retry later or switch to another channel.",
            StatusCode::BAD_GATEWAY,
            true,
        );
    }

    if status == StatusCode::BAD_REQUEST {
        return upstream_http_failure(
            status,
            detail,
            "upstream_bad_request",
            "upstream bad request",
            "The upstream provider rejected the request format or parameters. Please check the request and retry.",
            StatusCode::BAD_REQUEST,
            false,
        );
    }

    upstream_http_failure(
        status,
        detail,
        "upstream_http_error",
        "upstream http error",
        "The upstream provider rejected the request. Please retry later or switch to another channel.",
        StatusCode::BAD_GATEWAY,
        false,
    )
}

impl UpstreamHttpFailure {
    /// An exhausted upstream account cannot serve this request, but another selected upstream
    /// may still be healthy. This is distinct from asking the client to retry the request.
    pub(crate) fn should_failover(&self) -> bool {
        self.retryable || self.error_type == "upstream_quota_exhausted"
    }
}

fn upstream_http_failure(
    status: StatusCode,
    detail: String,
    error_type: &'static str,
    summary_prefix: &'static str,
    user_message: &'static str,
    relay_status: StatusCode,
    retryable: bool,
) -> UpstreamHttpFailure {
    UpstreamHttpFailure {
        error_type,
        user_message,
        summary: format!("{summary_prefix}: status {}; {detail}", status.as_u16()),
        detail,
        relay_status,
        retryable,
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
            "insufficient_quota",
            "insufficient quota",
            "exceeded your current quota",
            "quota exceeded",
            "insufficient balance",
            "insufficient credit",
            "not enough credits",
            "credit balance",
            "billing hard limit",
            "billing",
            "余额",
            "额度",
            "欠费",
        ],
    )
}

fn is_rate_limit_error(status: StatusCode, lowered: &str) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS
        || contains_any(
            lowered,
            &[
                "rate_limit_exceeded",
                "rate limit",
                "too many requests",
                "requests per minute",
                "tokens per minute",
                "overloaded_error",
                "overloaded",
                "请求过于频繁",
                "限流",
            ],
        )
}

fn is_auth_error(status: StatusCode, lowered: &str) -> bool {
    matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
        || contains_any(
            lowered,
            &[
                "invalid_api_key",
                "incorrect api key",
                "invalid api key",
                "expired api key",
                "authentication",
                "unauthorized",
                "permission denied",
                "forbidden",
                "access denied",
                "无效的 api key",
                "未授权",
                "无权限",
                "未登录",
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
            "model_not_found",
            "model not found",
            "model_not_available",
            "model is not available",
            "does not exist",
            "doesn't exist",
            "not supported",
            "unsupported model",
            "no such model",
            "unknown provider for model",
            "no provider for model",
            "provider for model",
            "模型不存在",
            "模型不可用",
            "不支持的模型",
        ],
    )
}

fn is_context_length_error(status: StatusCode, lowered: &str) -> bool {
    matches!(status, StatusCode::PAYLOAD_TOO_LARGE)
        || contains_any(
            lowered,
            &[
                "context_length_exceeded",
                "maximum context length",
                "context window",
                "too many tokens",
                "input is too long",
                "prompt is too long",
                "tokens exceeds",
                "上下文",
                "输入过长",
                "token 超",
            ],
        )
}

fn is_safety_error(lowered: &str) -> bool {
    contains_any(
        lowered,
        &[
            "content_policy_violation",
            "content policy",
            "safety",
            "moderation",
            "blocked",
            "sensitive content",
            "unsafe content",
            "内容安全",
            "安全策略",
            "敏感内容",
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

        assert_eq!(failure.error_type, "upstream_quota_exhausted");
        assert_eq!(failure.relay_status, StatusCode::BAD_GATEWAY);
        assert!(!failure.retryable);
        assert!(failure.should_failover());
        assert!(failure.summary.contains("insufficient_quota"));
    }

    #[test]
    fn classifies_chinese_balance_errors() {
        let failure =
            describe_upstream_http_failure(StatusCode::FORBIDDEN, "账户余额不足".as_bytes());

        assert_eq!(failure.error_type, "upstream_quota_exhausted");
        assert!(!failure.retryable);
        assert!(failure.should_failover());
    }

    #[test]
    fn classifies_rate_limit_errors_as_retryable() {
        let failure = describe_upstream_http_failure(
            StatusCode::TOO_MANY_REQUESTS,
            br#"{"error":{"message":"Rate limit reached"}}"#,
        );

        assert_eq!(failure.error_type, "upstream_rate_limited");
        assert_eq!(failure.relay_status, StatusCode::BAD_GATEWAY);
        assert!(failure.retryable);
    }

    #[test]
    fn classifies_context_length_as_bad_request() {
        let failure = describe_upstream_http_failure(
            StatusCode::BAD_REQUEST,
            br#"{"error":{"message":"maximum context length exceeded"}}"#,
        );

        assert_eq!(failure.error_type, "upstream_context_length_exceeded");
        assert_eq!(failure.relay_status, StatusCode::BAD_REQUEST);
        assert!(!failure.retryable);
    }

    #[test]
    fn classifies_authentication_errors() {
        let body =
            br#"{"error":{"message":"Incorrect API key provided","type":"invalid_api_key"}}"#;

        let failure = describe_upstream_http_failure(StatusCode::UNAUTHORIZED, body);

        assert_eq!(failure.error_type, "upstream_authentication_failed");
        assert_eq!(failure.relay_status, StatusCode::BAD_GATEWAY);
        assert!(!failure.retryable);
    }

    #[test]
    fn classifies_jdcloud_not_logged_in_as_auth_error() {
        // JDCloud's JoyAgent returns 406 with a JSON body whose `code` is 401
        // and `msg` is "账号未登录" when the channel secret is invalid or
        // expired. This must be treated as an authentication failure rather
        // than the generic upstream_http_error fallback.
        let body = "{\"code\":401,\"data\":null,\"msg\":\"账号未登录\"}".as_bytes();

        let failure = describe_upstream_http_failure(StatusCode::NOT_ACCEPTABLE, body);

        assert_eq!(failure.error_type, "upstream_authentication_failed");
        assert_eq!(failure.relay_status, StatusCode::BAD_GATEWAY);
        assert!(!failure.retryable);
        assert!(failure.detail.contains("账号未登录"));
    }

    #[test]
    fn classifies_model_errors() {
        let body =
            br#"{"error":{"message":"The model `gpt-x` does not exist","code":"model_not_found"}}"#;

        let failure = describe_upstream_http_failure(StatusCode::NOT_FOUND, body);

        assert_eq!(failure.error_type, "upstream_model_unavailable");
        assert_eq!(failure.relay_status, StatusCode::BAD_GATEWAY);
        assert!(!failure.retryable);
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

        assert_eq!(failure.error_type, "upstream_model_unavailable");
        assert_eq!(failure.relay_status, StatusCode::BAD_GATEWAY);
        assert!(!failure.retryable);
        assert!(failure
            .detail
            .contains("unknown provider for model gpt-5.2"));
    }

    #[test]
    fn classifies_server_errors_as_retryable() {
        let failure =
            describe_upstream_http_failure(StatusCode::INTERNAL_SERVER_ERROR, b"backend failed");

        assert_eq!(failure.error_type, "upstream_server_error");
        assert_eq!(failure.relay_status, StatusCode::BAD_GATEWAY);
        assert!(failure.retryable);
    }
}
