use clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID;
use oracle_ffi::{
    clock_address, price_account, query_price, ClockAddressRequest, OracleResult,
    PriceAccountRequest, QueryPriceRequest,
};

// The read surface must be reachable from the crate root — Rust callers import
// from `oracle_ffi::`, not `oracle_ffi::api`. clock_address needs no accounts,
// so exercise it directly; the snapshot readers need chain reads, so pin them
// with typed references.
#[test]
fn read_surface_is_reexported_from_crate_root() {
    let value = clock_address(ClockAddressRequest {}).expect("canonical clock address");
    assert_eq!(value["clockId"], CLOCK_01_PROGRAM_ACCOUNT_ID.to_string());
    assert_eq!(
        value["clockIdHex"],
        hex::encode(CLOCK_01_PROGRAM_ACCOUNT_ID.into_value())
    );
    let _: fn(PriceAccountRequest) -> OracleResult = price_account;
    let _: fn(QueryPriceRequest) -> OracleResult = query_price;
}
