use amm_client::{config_id, ConfigIdRequest, NEW_POSITION_SCHEMA};

#[test]
fn direct_rust_api_does_not_require_ffi() {
    let response = config_id(ConfigIdRequest {
        amm_program_id: "0000000000000000000000000000000000000000000000000000000000000000".into(),
    })
    .expect("valid program ID should produce a response");

    assert_eq!(response["status"], "ok");
    assert!(response["configId"].is_string());
    assert_eq!(NEW_POSITION_SCHEMA, "new-position.v1");
}
