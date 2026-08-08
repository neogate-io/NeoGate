use super::globalaiopc;
use super::haxicloud::{matches_base_url, HAXICLOUD_ADAPTER};
use super::{
    bailian::BAILIAN_ADAPTER, compatible::COMPATIBLE_ADAPTER, doubao::DOUBAO_ADAPTER,
    globalaiopc::GLOBALAIOPC_ADAPTER, jdcloud::JDCLOUD_ADAPTER, newapi::NEWAPI_ADAPTER,
    ProviderAdapter,
};

/// `hint` 优先于 `provider` 被用于选取 adapter，用于"openai 兼容"渠道等
/// provider 字段已不携带适配信息的场景。
pub(crate) fn adapter_for_endpoint(
    provider: &str,
    base_url: &str,
    hint: Option<&str>,
) -> &'static dyn ProviderAdapter {
    // hint 最优先：先于所有 URL/provider 检查，确保显式配置可覆盖自动检测。
    // 修复前：globalaiopc URL 匹配在 hint 之前，导致 hint=newapi 对 globalaiopc URL 无效。
    if let Some(h) = hint {
        if h.eq_ignore_ascii_case("newapi") {
            return &NEWAPI_ADAPTER;
        }
        // 未知 hint 值：记录 warn 便于排查配置拼写错误，而不是静默降级。
        tracing::warn!(
            hint = h,
            provider,
            base_url,
            "unknown adapter hint; falling through to provider/url detection"
        );
    }

    if provider.eq_ignore_ascii_case("openai") && globalaiopc::matches_base_url(base_url) {
        return &GLOBALAIOPC_ADAPTER;
    }
    if provider.eq_ignore_ascii_case("custom") && matches_base_url(base_url) {
        return &HAXICLOUD_ADAPTER;
    }
    if provider.eq_ignore_ascii_case("qwen") {
        &BAILIAN_ADAPTER
    } else if provider.eq_ignore_ascii_case("jdcloud") {
        &JDCLOUD_ADAPTER
    } else if provider.eq_ignore_ascii_case("doubao") {
        &DOUBAO_ADAPTER
    } else if provider.eq_ignore_ascii_case("newapi") {
        &NEWAPI_ADAPTER
    } else {
        &COMPATIBLE_ADAPTER
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_globalaiopc_only_for_exact_openai_host() {
        assert_eq!(
            adapter_for_endpoint("openai", "http://apillm.globalaiopc.com/gw_llm_power", None)
                .name(),
            "globalaiopc"
        );
        assert_eq!(
            adapter_for_endpoint(
                "openai",
                "http://apillm.globalaiopc.com.example.com/gw_llm_power",
                None
            )
            .name(),
            "compatible"
        );
        assert_eq!(
            adapter_for_endpoint("custom", "http://apillm.globalaiopc.com/gw_llm_power", None)
                .name(),
            "compatible"
        );
    }

    #[test]
    fn selects_haxicloud_only_for_matching_custom_host() {
        assert_eq!(
            adapter_for_endpoint("custom", "https://token.haxicloud.com", None).name(),
            "haxicloud"
        );
        assert_eq!(
            adapter_for_endpoint(
                "custom",
                "https://evil-token.haxicloud.com.example.com",
                None
            )
            .name(),
            "compatible"
        );
        assert_eq!(
            adapter_for_endpoint("doubao", "https://token.haxicloud.com", None).name(),
            "doubao"
        );
        assert_eq!(
            adapter_for_endpoint("NewAPI", "https://example.com", None).name(),
            "newapi"
        );
        // hint 优先于 provider
        assert_eq!(
            adapter_for_endpoint("openai", "https://example.com", Some("newapi")).name(),
            "newapi"
        );
    }
}
