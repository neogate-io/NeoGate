use super::{
    bailian::BAILIAN_ADAPTER, compatible::COMPATIBLE_ADAPTER, doubao::DOUBAO_ADAPTER,
    jdcloud::JDCLOUD_ADAPTER, ProviderAdapter,
};

pub(crate) fn adapter_for_provider(provider: &str) -> &'static dyn ProviderAdapter {
    if provider.eq_ignore_ascii_case("qwen") {
        &BAILIAN_ADAPTER
    } else if provider.eq_ignore_ascii_case("jdcloud") {
        &JDCLOUD_ADAPTER
    } else if provider.eq_ignore_ascii_case("doubao") {
        &DOUBAO_ADAPTER
    } else {
        &COMPATIBLE_ADAPTER
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_selects_bailian_for_qwen() {
        assert_eq!(adapter_for_provider("qwen").name(), "bailian");
        assert_eq!(adapter_for_provider("QWEN").name(), "bailian");
    }

    #[test]
    fn registry_selects_jdcloud_for_jdcloud() {
        assert_eq!(adapter_for_provider("jdcloud").name(), "jdcloud");
        assert_eq!(adapter_for_provider("JDCLOUD").name(), "jdcloud");
    }

    #[test]
    fn registry_selects_doubao_for_doubao_provider() {
        assert_eq!(adapter_for_provider("doubao").name(), "doubao");
        assert_eq!(adapter_for_provider("DOUBAO").name(), "doubao");
    }

    #[test]
    fn registry_defaults_to_compatible() {
        assert_eq!(adapter_for_provider("custom").name(), "compatible");
    }
}
