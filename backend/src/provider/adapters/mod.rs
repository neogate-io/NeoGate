mod bailian;
mod compatible;
mod jdcloud;
mod registry;
mod types;

pub(crate) use registry::adapter_for_provider;
pub(crate) use types::{AdapterResponseMode, PreparedUpstreamRequest, ProviderAdapter, RelayRoute};
