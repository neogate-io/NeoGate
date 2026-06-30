use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    auth::UserAuth,
    billing::{CreditAccountId, DebitHold, TokenUsage},
    id::DbId,
    relay::selector::{SelectedUpstream, UpstreamProtocol},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UpstreamTaskType {
    OpenAiResponse,
    NeogateResponse,
    AnthropicMessageBatch,
}

impl UpstreamTaskType {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::OpenAiResponse => "openai_response",
            Self::NeogateResponse => "neogate_response",
            Self::AnthropicMessageBatch => "anthropic_message_batch",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct NewUpstreamTask<'a> {
    pub(crate) task_type: UpstreamTaskType,
    pub(crate) upstream_task_id: &'a str,
    pub(crate) auth: &'a UserAuth,
    pub(crate) protocol: UpstreamProtocol,
    pub(crate) upstream: &'a SelectedUpstream,
    pub(crate) model: Option<&'a str>,
    pub(crate) upstream_model: Option<&'a str>,
    pub(crate) status: &'a str,
    pub(crate) terminal: bool,
    pub(crate) hold: &'a DebitHold,
    pub(crate) upstream_metadata: Value,
}

#[derive(Debug, Clone)]
pub(crate) struct UpstreamTask {
    pub(crate) id: DbId,
    pub(crate) task_type: UpstreamTaskType,
    pub(crate) upstream_task_id: String,
    pub(crate) user_id: DbId,
    pub(crate) project_id: DbId,
    pub(crate) user_key_id: DbId,
    pub(crate) provider: String,
    pub(crate) model: Option<String>,
    pub(crate) upstream_model: Option<String>,
    pub(crate) channel_id: DbId,
    pub(crate) channel_endpoint_id: DbId,
    pub(crate) channel_key_id: Option<DbId>,
    pub(crate) credential_id: Option<DbId>,
    pub(crate) upstream_base_url: String,
    pub(crate) status: String,
    pub(crate) terminal: bool,
    pub(crate) upstream_metadata: Value,
    pub(crate) created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub(crate) struct TaskBillingContext {
    pub(crate) user_id: DbId,
    pub(crate) project_id: DbId,
    pub(crate) user_key_id: DbId,
    pub(crate) project_credit_account: CreditAccountId,
    pub(crate) user_key_credit_account: CreditAccountId,
    pub(crate) user_key_model_credit_account: Option<CreditAccountId>,
    pub(crate) user_group: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct UsageSummary {
    pub(crate) input_tokens: Option<i64>,
    pub(crate) output_tokens: Option<i64>,
    pub(crate) total_tokens: Option<i64>,
}

impl UsageSummary {
    pub(crate) fn from_usage(usage: TokenUsage) -> Self {
        Self {
            input_tokens: Some(usage.input_tokens),
            output_tokens: Some(usage.output_tokens),
            total_tokens: Some(usage.total_tokens()),
        }
    }

    pub(crate) fn value_from_usage(usage: Option<TokenUsage>) -> serde_json::Result<Value> {
        usage
            .map(Self::from_usage)
            .map(serde_json::to_value)
            .transpose()
            .map(|value| value.unwrap_or_else(|| json!({})))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_summary_value_is_empty_without_usage() {
        assert_eq!(UsageSummary::value_from_usage(None).unwrap(), json!({}));
    }

    #[test]
    fn usage_summary_value_contains_token_totals() {
        let value = UsageSummary::value_from_usage(Some(TokenUsage {
            input_tokens: 3,
            output_tokens: 5,
            cached_input_tokens: None,
            cache_creation_input_tokens: None,
            cache_creation_input_tokens_5m: None,
            cache_creation_input_tokens_1h: None,
            reasoning_output_tokens: None,
            audio_input_tokens: None,
            audio_output_tokens: None,
        }))
        .unwrap();

        assert_eq!(value["input_tokens"], 3);
        assert_eq!(value["output_tokens"], 5);
        assert_eq!(value["total_tokens"], 8);
    }
}
