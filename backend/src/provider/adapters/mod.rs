pub(crate) mod bailian;
pub(crate) mod bailian_asr;
mod compatible;
mod doubao;
mod haxicloud;
mod jdcloud;
mod newapi;
mod registry;
mod types;

pub(crate) use registry::adapter_for_endpoint;
pub(crate) use types::{
    AdapterErrorDisposition, AdapterResponseMode, AnthropicCapabilities,
    PreparedResponseImageGenerationRequest, PreparedUpstreamRequest, ProviderAdapter,
    ProviderCapabilities, RelayRoute,
};
