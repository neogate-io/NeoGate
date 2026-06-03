mod apikeys;
mod overview;
mod recharge;
mod usage;

use std::sync::Arc;

use axum::Router;

use crate::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .merge(overview::router())
        .merge(usage::router())
        .merge(apikeys::router())
        .merge(recharge::router())
}
