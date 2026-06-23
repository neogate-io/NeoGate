use super::*;

#[test]
fn response_terminal_statuses_are_detected() {
    assert!(response_terminal("completed"));
    assert!(response_terminal("failed"));
    assert!(response_terminal("cancelled"));
    assert!(!response_terminal("in_progress"));
}

#[test]
fn response_query_streams_detects_true_flag() {
    assert!(response_query_streams(
        "/v1/responses/resp_123?starting_after=10&stream=true"
    ));
    assert!(!response_query_streams(
        "/v1/responses/resp_123?stream=false"
    ));
}

#[test]
fn response_subresource_path_uses_upstream_id_and_preserves_query() {
    let uri: Uri = "/v1/responses/resp_client/input_items?limit=20&after=item_1"
        .parse()
        .unwrap();

    let path = response_subresource_path("resp_upstream", &uri, "input_items");

    assert_eq!(
        path,
        "/v1/responses/resp_upstream/input_items?limit=20&after=item_1"
    );
}
