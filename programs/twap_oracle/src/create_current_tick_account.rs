use lee_core::{
    account::{AccountId, ShardData},
    program::AccountMeta,
};
use twap_oracle_core::{compute_current_tick_account_pda, price_to_tick, CurrentTickAccount};

/// Plan half: verify the current tick PDA and build its seed record.
///
/// The price source reports an opening spot **price** (`Q64.64` ratio), converted
/// to a tick via [`price_to_tick`], so the source never needs to know about ticks.
///
/// Authorization is implicit in the PDA relationship: the current tick account is
/// derived from `price_source.account_id`, so whoever controls the price source
/// controls this account.
///
/// # Panics
/// Panics if:
/// - `current_tick_account.account_id` does not match
///   `compute_current_tick_account_pda(oracle_program_id, price_source.account_id)`.
/// - `price_source.is_authorized` is false.
pub fn plan_create_current_tick_account(
    current_tick_account: &AccountMeta,
    price_source: &AccountMeta,
    initial_price: u128,
    now: u64,
    oracle_program_id: AccountId,
) -> ShardData {
    assert_eq!(
        current_tick_account.account_id,
        compute_current_tick_account_pda(oracle_program_id, price_source.account_id),
        "CreateCurrentTickAccount: current tick account ID does not match expected PDA"
    );
    assert!(
        price_source.is_authorized,
        "CreateCurrentTickAccount: price source account must be authorized"
    );

    ShardData::from(&CurrentTickAccount {
        tick: price_to_tick(initial_price),
        last_updated: now,
    })
}
