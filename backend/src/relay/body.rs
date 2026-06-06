use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::{FromRequest, Request},
    http::{header, StatusCode},
};

use crate::{error::AppError, AppState};

pub(super) struct RelayBody(pub(super) Bytes);

impl FromRequest<Arc<AppState>> for RelayBody {
    type Rejection = AppError;

    async fn from_request(req: Request, state: &Arc<AppState>) -> Result<Self, Self::Rejection> {
        let method = req.method().clone();
        let path = req.uri().path().to_string();
        let content_length = req
            .headers()
            .get(header::CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok());

        match Bytes::from_request(req, state).await {
            Ok(body) => Ok(Self(body)),
            Err(rejection) => {
                let status = rejection.status();
                let message = rejection.body_text();
                tracing::warn!(
                    %method,
                    %path,
                    status = status.as_u16(),
                    ?content_length,
                    relay_body_limit_bytes = state.config.relay_body_limit_bytes,
                    rejection = %message,
                    "relay request body rejected"
                );
                if status == StatusCode::PAYLOAD_TOO_LARGE {
                    return Err(AppError::PayloadTooLarge(format!(
                        "request body exceeds {} bytes",
                        state.config.relay_body_limit_bytes
                    )));
                }
                Err(AppError::BadRequest(message))
            }
        }
    }
}
