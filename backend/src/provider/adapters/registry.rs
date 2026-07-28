use super::{
    bailian::BAILIAN_ADAPTER,
    compatible::COMPATIBLE_ADAPTER,
    doubao::DOUBAO_ADAPTER,
    haxicloud::{matches_base_url, HAXICLOUD_ADAPTER},
    jdcloud::JDCLOUD_ADAPTER,
    newapi::NEWAPI_ADAPTER,
    ProviderAdapter,
};

pub(crate) fn adapter_for_endpoint(provider: &str, base_url: &str) -> &'static dyn ProviderAdapter {
    if provider.eq_ignore_ascii_case("custom") && matches_base_url(base_url) {
        &HAXICLOUD_ADAPTER
    } else if provider.eq_ignore_ascii_case("qwen") {
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
    fn selects_haxicloud_only_for_matching_custom_host() {
        assert_eq!(
            adapter_for_endpoint("custom", "https://token.haxicloud.com").name(),
            "haxicloud"
        );
        assert_eq!(
            adapter_for_endpoint("custom", "https://evil-token.haxicloud.com.example.com").name(),
            "compatible"
        );
        assert_eq!(
            adapter_for_endpoint("doubao", "https://token.haxicloud.com").name(),
            "doubao"
        );
        assert_eq!(
            adapter_for_endpoint("NewAPI", "https://example.com").name(),
            "newapi"
        );
    }
}
