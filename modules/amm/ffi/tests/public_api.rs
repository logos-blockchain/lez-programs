use amm_ffi::{
    config_id, create_pool_plan, create_pool_quote, AmmResult, ConfigIdRequest,
    CreatePoolPlanRequest, CreatePoolQuoteRequest,
};

#[test]
fn direct_rust_api_does_not_require_ffi() {
    let response = config_id(ConfigIdRequest {
        amm_program_id: "0000000000000000000000000000000000000000000000000000000000000000".into(),
    })
    .expect("valid program ID should produce a response");

    assert_eq!(response["status"], "ok");
    assert!(response["configId"].is_string());
}

// The create-pool surface must be reachable from the crate root too — Rust callers import from
// `amm_ffi::`, not `amm_ffi::api`. create_pool_quote is a pure preview, so exercise it directly;
// create_pool_plan needs chain reads, so a typed reference is enough to pin the re-export.
#[test]
fn create_pool_surface_is_reexported_from_crate_root() {
    let quote = create_pool_quote(CreatePoolQuoteRequest {
        token_a_id: "11".repeat(32),
        token_b_id: "22".repeat(32),
        price_raw: None, // amounts supplied ⇒ the op derives the price
        amount_a_raw: Some("1000000".into()),
        amount_b_raw: Some("4000000".into()),
    })
    .expect("a valid pure create-pool quote should succeed");
    assert_eq!(quote["actualAmountARaw"], "1000000");

    let _plan: fn(CreatePoolPlanRequest) -> AmmResult = create_pool_plan;
}
