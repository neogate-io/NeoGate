use std::sync::Arc;

use axum::Router;

use crate::AppState;

pub(crate) mod bailian;
pub(crate) mod bailian_asr;
mod compatible;
mod doubao;
mod globalaiopc;
mod haxicloud;
mod jdcloud;
mod newapi;
mod registry;
mod types;

pub(crate) use registry::adapter_for_endpoint;
pub(crate) use types::{
    openai_video_task_id, AdapterErrorDisposition, AdapterResponseMode, AnthropicCapabilities,
    AssetCreateRequest, AssetType, NormalizedAsset, PreparedResponseImageGenerationRequest,
    PreparedUpstreamRequest, ProviderAdapter, ProviderCapabilities, RelayRoute,
};

pub(crate) fn router() -> Router<Arc<AppState>> {
    let router = Router::new();
    router.merge(haxicloud::router())
}
