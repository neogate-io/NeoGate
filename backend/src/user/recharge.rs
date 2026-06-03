use std::sync::Arc;

use axum::{extract::State, routing::get, Json, Router};

use crate::{
    auth::UserSessionAuth,
    error::AppResult,
    payment::{
        create_user_payment_order, list_user_payment_orders, CreatePaymentOrderRequest,
        CreatePaymentOrderResponse, PaymentOrderRecord,
    },
    AppState,
};

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route(
        "/api/user/recharge/orders",
        get(list_orders).post(create_order),
    )
}

async fn create_order(
    State(state): State<Arc<AppState>>,
    auth: UserSessionAuth,
    Json(req): Json<CreatePaymentOrderRequest>,
) -> AppResult<Json<CreatePaymentOrderResponse>> {
    Ok(Json(create_user_payment_order(&state, auth, req).await?))
}

async fn list_orders(
    State(state): State<Arc<AppState>>,
    auth: UserSessionAuth,
) -> AppResult<Json<Vec<PaymentOrderRecord>>> {
    Ok(Json(list_user_payment_orders(&state, auth).await?))
}
