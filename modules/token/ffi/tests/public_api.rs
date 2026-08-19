use token_ffi::{
    burn_plan, create_fungible_plan, decode_definition, print_nft_plan, transfer_plan,
    BurnPlanRequest, CreateFungiblePlanRequest, DecodeDefinitionRequest, PrintNftPlanRequest,
    TokenResult, TransferPlanRequest,
};

#[test]
fn crate_root_reexports_token_surface() {
    let _decode: fn(DecodeDefinitionRequest) -> TokenResult = decode_definition;
    let _create: fn(CreateFungiblePlanRequest) -> TokenResult = create_fungible_plan;
    let _transfer: fn(TransferPlanRequest) -> TokenResult = transfer_plan;
    let _burn: fn(BurnPlanRequest) -> TokenResult = burn_plan;
    let _print: fn(PrintNftPlanRequest) -> TokenResult = print_nft_plan;
}
