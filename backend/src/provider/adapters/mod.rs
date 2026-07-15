mod bailian;
mod compatible;
mod doubao;
mod haxicloud;
mod jdcloud;
mod newapi;
mod registry;
mod types;

pub(crate) use registry::adapter_for_endpoint;
pub(crate) use types::{
    AdapterErrorDisposition, AdapterResponseMode, PreparedHttpRetry,
    PreparedResponseImageGenerationRequest, PreparedUpstreamRequest, ProviderAdapter,
    ProviderCapabilities, RelayRoute,
};
