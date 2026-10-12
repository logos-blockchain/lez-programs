use lee_core::{
    account::{AccountId, ShardData},
    program::AccountMeta,
};
use twap_oracle_core::{
    compute_oracle_price_account_pda, OraclePriceAccount, OBSERVATIONS_CAPACITY,
};

/// Plan half: verify the price account's PDA and build its seed record.
///
/// Both `initial_price` and `now` must be non-zero: a zero price or zero
/// timestamp is the sentinel consumers treat as "no valid price", so an account
/// must never be created in that state. `confidence_interval` starts at zero.
///
/// `initial_price` is a `Q64.64` fixed-point value: the real price is
/// `initial_price / 2^64`, so `1.0` is `1 << 64`. The non-zero check rejects the
/// sentinel but cannot validate scale — a caller is responsible for supplying a
/// correctly-scaled fixed-point price, not a plain integer.
///
/// # Panics
/// Panics if:
/// - `oracle_price_account.account_id` does not match
///   `compute_oracle_price_account_pda(oracle_program_id, price_source.account_id,
///   window_duration)`.
/// - `price_source.is_authorized` is false (caller does not control the price source account).
/// - `initial_price` or `now` is zero.
/// - `window_duration` is smaller than [`OBSERVATIONS_CAPACITY`]. Such a window can never have a
///   matching `PriceObservations` account (`CreatePriceObservations` rejects it for the same
///   reason), so `PublishPrice` could never update this account — it would be created frozen at its
///   seed price and squat its PDA forever. Rejecting it here keeps both creation paths symmetric.
#[expect(
    clippy::too_many_arguments,
    reason = "instruction surface passes explicit account inputs alongside the asset pair, initial price, window and clock reading"
)]
pub fn plan_create_oracle_price_account(
    oracle_price_account: &AccountMeta,
    price_source: &AccountMeta,
    base_asset: AccountId,
    quote_asset: AccountId,
    initial_price: u128,
    window_duration: u64,
    now: u64,
    oracle_program_id: AccountId,
) -> ShardData {
    let price_source_id = price_source.account_id;
    assert_eq!(
        oracle_price_account.account_id,
        compute_oracle_price_account_pda(oracle_program_id, price_source_id, window_duration),
        "CreateOraclePriceAccount: oracle price account ID does not match expected PDA"
    );
    assert!(
        price_source.is_authorized,
        "CreateOraclePriceAccount: price source account must be authorized (caller must control it via a PDA)"
    );
    assert!(
        window_duration >= u64::from(OBSERVATIONS_CAPACITY),
        "CreateOraclePriceAccount: window_duration must be >= OBSERVATIONS_CAPACITY so a matching \
         PriceObservations account can exist and PublishPrice can update this price account"
    );
    assert!(
        initial_price != 0,
        "CreateOraclePriceAccount: initial price must be non-zero"
    );
    assert!(
        now != 0,
        "CreateOraclePriceAccount: clock timestamp must be non-zero"
    );

    ShardData::from(&OraclePriceAccount {
        base_asset,
        quote_asset,
        price: initial_price,
        timestamp: now,
        source_id: price_source_id,
        confidence_interval: 0,
    })
}
