use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
}
