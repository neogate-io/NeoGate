pub(crate) const PROVIDER_CODE: &str = "newapi";

pub(crate) fn is_newapi_provider(provider: &str) -> bool {
    provider.eq_ignore_ascii_case(PROVIDER_CODE)
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ImageRequestCompat {
    pub(crate) accept_event_stream: bool,
}

pub(crate) fn image_request_compat(provider: &str, stream: bool) -> ImageRequestCompat {
    if is_newapi_provider(provider) {
        return ImageRequestCompat {
            accept_event_stream: stream,
        };
    }
    ImageRequestCompat {
        accept_event_stream: stream,
    }
}
