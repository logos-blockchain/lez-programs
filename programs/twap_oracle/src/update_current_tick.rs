use lee_core::{
    account::{AccountId, ShardData},
    program::AccountMeta,
};
use twap_oracle_core::{compute_current_tick_account_pda, price_to_tick, CurrentTickAccount};

/// Plan half: verify the PDA and the price source's authority, and convert the
/// reported spot price to a tick.
///
/// The conversion stays in the plan deliberately: it is a pure function of an
/// instruction field, so the apply half has nothing to re-derive and the IDL keeps
/// describing the instruction in the terms the price source speaks (prices).
///
/// # Panics
/// Panics if:
/// - `current_tick_account.account_id` does not match
///   `compute_current_tick_account_pda(oracle_program_id, price_source.account_id)`.
/// - `price_source.is_authorized` is false.
pub fn plan_update_current_tick(
    current_tick_account: &AccountMeta,
    price_source: &AccountMeta,
    price: u128,
    oracle_program_id: AccountId,
) -> i32 {
    assert_eq!(
        current_tick_account.account_id,
        compute_current_tick_account_pda(oracle_program_id, price_source.account_id),
        "UpdateCurrentTick: current tick account ID does not match expected PDA"
    );
    assert!(
        price_source.is_authorized,
        "UpdateCurrentTick: price source account must be authorized"
    );

    price_to_tick(price)
}

/// Apply half: overwrite the live tick and its timestamp.
///
/// The shard must already hold a `CurrentTickAccount` — this instruction updates,
/// it does not create. Decoding is the check; an empty or foreign shard fails.
///
/// # Panics
/// If the shard does not hold a valid [`CurrentTickAccount`].
#[must_use]
pub fn write_current_tick(pre_data: &ShardData, tick: i32, last_updated: u64) -> ShardData {
    let mut stored = CurrentTickAccount::try_from(pre_data)
        .expect("UpdateCurrentTick: current tick account must be initialized");
    stored.tick = tick;
    stored.last_updated = last_updated;
    ShardData::from(&stored)
}

/// Guard on the `CurrentTickAccount` shard: it must hold exactly these values.
///
/// This is what turns `RecordTick`'s and `PublishPrice`'s declared `current_tick`
/// into a fact. Neither of those writes this shard; they integrate or extrapolate
/// from it, and their own applies cannot see it.
///
/// # Panics
/// If the shard is not a valid [`CurrentTickAccount`], or disagrees with the
/// declared values.
pub fn check_current_tick(pre_data: &ShardData, tick: i32, last_updated: Option<u64>) {
    let stored =
        CurrentTickAccount::try_from(pre_data).expect("current tick account must be initialized");
    assert_eq!(
        stored.tick, tick,
        "Declared current tick does not match the current tick account"
    );
    if let Some(last_updated) = last_updated {
        assert_eq!(
            stored.last_updated, last_updated,
            "Declared current tick timestamp does not match the current tick account"
        );
    }
}
