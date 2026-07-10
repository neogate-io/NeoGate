use std::collections::HashMap;

use crate::{
    config::ZpayConfig,
    error::{AppError, AppResult},
};
use axum::{body::Bytes, http::HeaderMap};
use md5::{Digest, Md5};

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
        let out_trade_no = req.order_no.to_string();
        let Some(return_url) = req.return_url else {
            return Err(AppError::BadRequest(
                "return_url is required for ZPAY checkout".to_string(),
            ));
        };
        let mut params = vec![
            ("money".to_string(), money),
            ("name".to_string(), req.subject),
            ("notify_url".to_string(), req.notify_url),
            ("out_trade_no".to_string(), out_trade_no),
            ("pid".to_string(), merchant_id.to_string()),
            ("return_url".to_string(), return_url),
            ("sitename".to_string(), self.config.site_name.clone()),
            (
                "type".to_string(),
                req.pay_type
                    .unwrap_or_else(|| self.config.default_pay_type.clone()),
            ),
        ];
        params.push((
            "sign".to_string(),
            sign_pairs(
                params
                    .iter()
                    .map(|(name, value)| (name.as_str(), value.as_str()))
                    .collect(),
                self.secret_key()?,
            ),
        ));
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
        parse_signed_notification(
            params,
            self.secret_key()?,
            self.config.merchant_id.as_deref(),
        )
    }
}

fn parse_signed_notification(
    params: HashMap<String, String>,
    key: &str,
    merchant_id: Option<&str>,
) -> AppResult<GatewayNotification> {
    verify_sign(&params, key)?;
    verify_merchant_id(&params, merchant_id)?;
    let raw_order_id = params
        .get("out_trade_no")
        .ok_or_else(|| AppError::BadRequest("missing out_trade_no".to_string()))?;
    let order_no = raw_order_id
        .parse::<i64>()
        .map_err(|_| AppError::BadRequest("invalid out_trade_no".to_string()))?;
    let provider_order_id = params
        .get("trade_no")
        .or_else(|| params.get("provider_order_id"))
        .cloned();
    let payable_amount_minor = params
        .get("money")
        .and_then(|value| parse_money_minor(value));
    let status = zpay_trade_status(params.get("trade_status"))?;
    Ok(GatewayNotification {
        order_no,
        provider_order_id,
        payable_amount_minor,
        status,
        payload: payload_json(&params),
    })
}

fn verify_merchant_id(
    params: &HashMap<String, String>,
    merchant_id: Option<&str>,
) -> AppResult<()> {
    let Some(expected_pid) = merchant_id else {
        return Ok(());
    };
    let Some(actual_pid) = params.get("pid") else {
        return Err(AppError::BadRequest("missing ZPAY merchant ID".to_string()));
    };
    if actual_pid != expected_pid {
        return Err(AppError::BadRequest(
            "ZPAY merchant ID does not match".to_string(),
        ));
    }
    Ok(())
}

fn zpay_trade_status(value: Option<&String>) -> AppResult<PaymentStatus> {
    let Some(value) = value else {
        return Err(AppError::BadRequest(
            "missing ZPAY trade_status".to_string(),
        ));
    };
    if value == "TRADE_SUCCESS" {
        return Ok(PaymentStatus::Paid);
    }
    Ok(PaymentStatus::Pending)
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
    let pairs = params
        .iter()
        .filter(|(name, value)| {
            name.as_str() != "sign" && name.as_str() != "sign_type" && !value.is_empty()
        })
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    sign_pairs(pairs, key)
}

fn sign_pairs(mut pairs: Vec<(&str, &str)>, key: &str) -> String {
    pairs.sort_unstable_by_key(|(name, _)| *name);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn signed_notify_params() -> HashMap<String, String> {
        let mut params = HashMap::from([
            ("money".to_string(), "1.20".to_string()),
            ("name".to_string(), "账户充值".to_string()),
            ("out_trade_no".to_string(), "10000001".to_string()),
            ("pid".to_string(), "1001".to_string()),
            ("trade_no".to_string(), "20260621123456789".to_string()),
            ("trade_status".to_string(), "TRADE_SUCCESS".to_string()),
            ("type".to_string(), "alipay".to_string()),
        ]);
        let sign = sign_map(&params, "secret");
        params.insert("sign".to_string(), sign);
        params.insert("sign_type".to_string(), "MD5".to_string());
        params
    }

    #[test]
    fn zpay_signature_ignores_sign_type_and_empty_values() {
        let mut params = HashMap::from([
            ("b".to_string(), "2".to_string()),
            ("a".to_string(), "1".to_string()),
            ("empty".to_string(), "".to_string()),
            ("sign_type".to_string(), "MD5".to_string()),
            ("sign".to_string(), "ignored".to_string()),
        ]);

        let expected = {
            let mut hasher = Md5::new();
            hasher.update("a=1&b=2secret".as_bytes());
            hex::encode(hasher.finalize())
        };

        assert_eq!(sign_map(&params, "secret"), expected);
        params.insert("sign".to_string(), expected);
        assert!(verify_sign(&params, "secret").is_ok());
    }

    #[test]
    fn zpay_notification_parses_paid_order() {
        let notification =
            parse_signed_notification(signed_notify_params(), "secret", Some("1001")).unwrap();

        assert_eq!(notification.order_no, 10000001);
        assert_eq!(
            notification.provider_order_id.as_deref(),
            Some("20260621123456789")
        );
        assert_eq!(notification.payable_amount_minor, Some(120));
        assert_eq!(notification.status, PaymentStatus::Paid);
    }

    #[test]
    fn zpay_notification_rejects_wrong_merchant() {
        let err = parse_signed_notification(signed_notify_params(), "secret", Some("1002"))
            .expect_err("merchant ID should be validated");

        assert!(err.to_string().contains("merchant ID"));
    }

    #[test]
    fn zpay_notification_requires_trade_success() {
        let mut params = signed_notify_params();
        params.insert("trade_status".to_string(), "WAIT_BUYER_PAY".to_string());
        let sign = sign_map(&params, "secret");
        params.insert("sign".to_string(), sign);

        let notification = parse_signed_notification(params, "secret", Some("1001")).unwrap();

        assert_eq!(notification.status, PaymentStatus::Pending);
    }
}
