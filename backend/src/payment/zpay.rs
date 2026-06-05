use std::collections::HashMap;

use axum::{body::Bytes, http::HeaderMap};
use md5::{Digest, Md5};
use uuid::Uuid;

use crate::{
    config::ZpayConfig,
    error::{AppError, AppResult},
};

use super::{
    form_or_json_payload, payload_json, GatewayCreateRequest, GatewayCreateResponse,
    GatewayNotification, PaymentGateway, PaymentStatus,
};

pub(super) struct ZpayGateway {
    config: ZpayConfig,
}

impl ZpayGateway {
    pub(super) fn new(config: ZpayConfig) -> AppResult<Self> {
        config.require_ready()?;
        Ok(Self { config })
    }

    fn secret_key(&self) -> AppResult<&str> {
        self.config
            .secret_key
            .as_deref()
            .ok_or_else(|| AppError::BadRequest("ZPAY secret key is required".to_string()))
    }
}

impl PaymentGateway for ZpayGateway {
    fn create_checkout(&self, req: GatewayCreateRequest) -> AppResult<GatewayCreateResponse> {
        let api_url = self
            .config
            .api_url
            .as_deref()
            .ok_or_else(|| AppError::BadRequest("ZPAY API URL is required".to_string()))?;
        let merchant_id = self
            .config
            .merchant_id
            .as_deref()
            .ok_or_else(|| AppError::BadRequest("ZPAY merchant ID is required".to_string()))?;
        let money = format!("{:.2}", req.payable_amount_minor as f64 / 100.0);
        let out_trade_no = req.order_id.simple().to_string();
        let Some(return_url) = req.return_url else {
            return Err(AppError::BadRequest(
                "return_url is required for ZPAY checkout".to_string(),
            ));
        };
        let mut params = vec![
            ("money".to_string(), money),
            ("name".to_string(), req.subject),
            ("notify_url".to_string(), req.notify_url),
            ("out_trade_no".to_string(), out_trade_no.clone()),
            ("pid".to_string(), merchant_id.to_string()),
            ("return_url".to_string(), return_url),
            ("sitename".to_string(), self.config.site_name.clone()),
            (
                "type".to_string(),
                req.pay_type
                    .unwrap_or_else(|| self.config.default_pay_type.clone()),
            ),
        ];
        params.push(("sign".to_string(), sign_pairs(&params, self.secret_key()?)));
        params.push(("sign_type".to_string(), "MD5".to_string()));

        let query = serde_urlencoded::to_string(&params).map_err(|err| {
            AppError::BadRequest(format!("failed to build ZPAY checkout URL: {err}"))
        })?;
        let separator = if api_url.contains('?') { '&' } else { '?' };
        Ok(GatewayCreateResponse {
            provider_order_id: None,
            checkout_url: Some(format!("{api_url}{separator}{query}")),
        })
    }

    fn parse_notification(
        &self,
        headers: &HeaderMap,
        body: &Bytes,
    ) -> AppResult<GatewayNotification> {
        self.parse_query_notification(form_or_json_payload(headers, body)?)
    }

    fn parse_query_notification(
        &self,
        params: HashMap<String, String>,
    ) -> AppResult<GatewayNotification> {
        parse_signed_notification(params, self.secret_key()?)
    }
}

fn parse_signed_notification(
    params: HashMap<String, String>,
    key: &str,
) -> AppResult<GatewayNotification> {
    verify_sign(&params, key)?;
    let raw_order_id = params
        .get("out_trade_no")
        .ok_or_else(|| AppError::BadRequest("missing out_trade_no".to_string()))?;
    let order_id = Uuid::parse_str(raw_order_id)
        .map_err(|_| AppError::BadRequest("invalid out_trade_no".to_string()))?;
    let provider_order_id = params
        .get("trade_no")
        .or_else(|| params.get("provider_order_id"))
        .cloned();
    let payable_amount_minor = params
        .get("money")
        .and_then(|value| parse_money_minor(value));
    let status = PaymentStatus::from_gateway_value(
        params
            .get("trade_status")
            .or_else(|| params.get("status"))
            .or_else(|| params.get("state")),
    );
    Ok(GatewayNotification {
        order_id,
        provider_order_id,
        payable_amount_minor,
        status,
        payload: payload_json(&params),
    })
}

fn parse_money_minor(value: &str) -> Option<i64> {
    let trimmed = value.trim();
    let (whole, fraction) = trimmed.split_once('.').unwrap_or((trimmed, ""));
    let whole = whole.parse::<i64>().ok()?;
    let cents = match fraction.len() {
        0 => 0,
        1 => fraction.parse::<i64>().ok()? * 10,
        _ => fraction.get(..2)?.parse::<i64>().ok()?,
    };
    Some(whole * 100 + cents)
}

fn verify_sign(params: &HashMap<String, String>, key: &str) -> AppResult<()> {
    let Some(sign) = params.get("sign") else {
        return Err(AppError::BadRequest(
            "missing payment notification signature".to_string(),
        ));
    };
    let expected = sign_map(params, key);
    if !sign.eq_ignore_ascii_case(&expected) {
        return Err(AppError::BadRequest(
            "invalid payment notification signature".to_string(),
        ));
    }
    Ok(())
}

fn sign_map(params: &HashMap<String, String>, key: &str) -> String {
    let pairs: Vec<_> = params
        .iter()
        .filter(|(key, value)| {
            key.as_str() != "sign" && key.as_str() != "sign_type" && !value.is_empty()
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    sign_pairs(&pairs, key)
}

fn sign_pairs(pairs: &[(String, String)], key: &str) -> String {
    let mut pairs = pairs.to_vec();
    pairs.sort_by(|left, right| left.0.cmp(&right.0));
    let payload = pairs
        .iter()
        .filter(|(_, value)| !value.is_empty())
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("&");
    let mut hasher = Md5::new();
    hasher.update(format!("{payload}{key}").as_bytes());
    hex::encode(hasher.finalize())
}
