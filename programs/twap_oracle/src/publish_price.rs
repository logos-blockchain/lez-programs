use lee_core::{
    account::{AccountId, ShardData},
    program::AccountMeta,
};
use twap_oracle_core::{
    compute_current_tick_account_pda, compute_oracle_price_account_pda,
    compute_price_observations_pda, compute_twap_price, OraclePriceAccount, PriceObservations,
};

/// Plan half: verify all three PDAs against `price_source_id`.
///
/// The TWAP itself cannot be computed here — it needs the ring buffer, the live
/// tick and the clock, none of which the plan phase can read. So the caller
/// computes it and declares it, and [`check_twap`] below recomputes it where the
/// buffer actually is.
///
/// # Panics
/// Panics if any account ID does not match its expected PDA derivation.
pub fn plan_publish_price(
    price_observations: &AccountMeta,
    oracle_price_account: &AccountMeta,
    current_tick_account: &AccountMeta,
    price_source_id: AccountId,
    window_duration: u64,
    oracle_program_id: AccountId,
) {
    assert_eq!(
        price_observations.account_id,
        compute_price_observations_pda(oracle_program_id, price_source_id, window_duration),
        "PublishPrice: price observations account ID does not match expected PDA"
    );
    assert_eq!(
        oracle_price_account.account_id,
        compute_oracle_price_account_pda(oracle_program_id, price_source_id, window_duration),
        "PublishPrice: oracle price account ID does not match expected PDA"
    );
    assert_eq!(
        current_tick_account.account_id,
        compute_current_tick_account_pda(oracle_program_id, price_source_id),
        "PublishPrice: current tick account ID does not match expected PDA"
    );
}

/// Guard on the observations shard: the declared TWAP must be what this ring
/// buffer yields.
///
/// This is where the whole instruction's integrity rests. `tick`, `last_updated`
/// and `now` arrive declared, each pinned by its own guard on the shard it came
/// from; this apply holds the one input nobody else can see — the buffer — and
/// recomputes the average from all four. A caller who lies about any of them
/// fails either this assertion or the guard that pins that value.
///
/// `None` is the claim "fewer than two observations, nothing to publish", and is
/// checked just as strictly: a caller cannot suppress a publish that is due, nor
/// force one that is not.
///
/// # Panics
/// If the shard is not valid [`PriceObservations`], or the recomputed TWAP differs
/// from the declared one.
pub fn check_twap(
    pre_data: &ShardData,
    tick: i32,
    last_updated: u64,
    now: u64,
    twap_price: Option<u128>,
) {
    let observations = PriceObservations::try_from(pre_data)
        .expect("PublishPrice: price observations account must be initialized");

    assert_eq!(
        compute_twap_price(&observations, tick, last_updated, now),
        twap_price,
        "Declared TWAP does not match the price observations"
    );
}

/// Apply half on the price account: publish a price a [`check_twap`] guard pinned.
///
/// Only `price` and `timestamp` move; the asset pair, source and confidence
/// interval are whatever the account was created with.
///
/// # Panics
/// If the shard does not hold a valid [`OraclePriceAccount`].
#[must_use]
pub fn publish_twap(pre_data: &ShardData, price: u128, now: u64) -> ShardData {
    let mut price_account = OraclePriceAccount::try_from(pre_data)
        .expect("PublishPrice: oracle price account must be initialized");
    price_account.price = price;
    price_account.timestamp = now;
    ShardData::from(&price_account)
}
