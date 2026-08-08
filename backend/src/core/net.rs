use crate::error::{AppError, AppResult};

/// 校验 URL 必须使用公开的 http/https 地址，阻断内网/回环/元数据端点，防止 SSRF。
///
/// 拦截范围：
/// - 非 http/https scheme
/// - localhost / .local / .localhost 等主机名
/// - 10.0.0.0/8、172.16.0.0/12、192.168.0.0/16（RFC 1918 内网）
/// - 169.254.0.0/16（AWS/GCP/Azure 实例元数据端点 169.254.169.254 等）
/// - ::1 / fe80::/10（IPv6 链路本地）/ fc00::/7（IPv6 唯一本地）
pub(crate) fn validate_public_url(value: &str) -> AppResult<()> {
    let url = reqwest::Url::parse(value)
        .map_err(|_| AppError::BadRequest("url must be a valid URL".into()))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(AppError::BadRequest(
            "url must use a public http or https URL".into(),
        ));
    }
    // host_str() 对 IPv6 地址会保留方括号（如 "[::1]"），所以主机名检查用它，
    // IP 路由检查改用 url.host() 枚举，可以直接拿到 Ipv4Addr / Ipv6Addr。
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    if matches!(host.as_str(), "localhost" | "127.0.0.1" | "0.0.0.0")
        || host.ends_with(".localhost")
        || host.ends_with(".local")
    {
        return Err(AppError::BadRequest("url must use a public host".into()));
    }
    // url.host_str() 对 IPv6 会保留方括号（如 "[::1]"），去掉方括号后再解析 IP。
    // 这样可以正确拦截 ::1 等 IPv6 回环/链路本地/唯一本地地址。
    let host_for_ip = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(addr) = host_for_ip.parse::<std::net::IpAddr>() {
        if !is_globally_routable_ip(addr) {
            return Err(AppError::BadRequest("url must use a public host".into()));
        }
    }
    Ok(())
}

/// 判断一个 IP 地址是否属于可公开路由的范围。
/// 阻断环回、私有、链路本地、广播及文档用地址，防止 SSRF。
pub(crate) fn is_globally_routable_ip(addr: std::net::IpAddr) -> bool {
    match addr {
        std::net::IpAddr::V4(v4) => {
            // is_private():   10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
            // is_link_local(): 169.254.0.0/16（含 AWS 元数据端点 169.254.169.254）
            !v4.is_private()
                && !v4.is_loopback()
                && !v4.is_link_local()
                && !v4.is_broadcast()
                && !v4.is_unspecified()
                && !v4.is_documentation()
        }
        std::net::IpAddr::V6(v6) => {
            let octets = v6.octets();
            // fe80::/10（链路本地）
            let is_link_local = octets[0] == 0xfe && (octets[1] & 0xc0 == 0x80);
            // fc00::/7（唯一本地，含 fd00::/8）
            let is_unique_local = octets[0] & 0xfe == 0xfc;
            !v6.is_loopback() && !v6.is_unspecified() && !is_link_local && !is_unique_local
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_localhost_variants() {
        assert!(validate_public_url("http://localhost/path").is_err());
        assert!(validate_public_url("http://127.0.0.1/path").is_err());
        assert!(validate_public_url("http://0.0.0.0/path").is_err());
        assert!(validate_public_url("http://[::1]/path").is_err());
        assert!(validate_public_url("http://app.localhost/path").is_err());
        assert!(validate_public_url("http://app.local/path").is_err());
    }

    #[test]
    fn blocks_private_ip_ranges() {
        assert!(validate_public_url("http://10.0.0.1/path").is_err());
        assert!(validate_public_url("http://172.16.0.1/path").is_err());
        assert!(validate_public_url("http://192.168.1.1/path").is_err());
    }

    #[test]
    fn blocks_link_local_metadata_endpoint() {
        assert!(validate_public_url("http://169.254.169.254/latest/meta-data/").is_err());
    }

    #[test]
    fn blocks_non_http_schemes() {
        assert!(validate_public_url("ftp://example.com/file").is_err());
        assert!(validate_public_url("file:///etc/passwd").is_err());
    }

    #[test]
    fn allows_public_urls() {
        assert!(validate_public_url("https://oapi.dingtalk.com/robot/sendBySession?session=x").is_ok());
        assert!(validate_public_url("https://example.com/path").is_ok());
        assert!(validate_public_url("http://93.184.216.34/path").is_ok());
    }
}
