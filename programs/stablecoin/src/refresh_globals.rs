//! `Instruction::RefreshGlobals` (spec §10.3a).
//!
//! Best-effort combined poke: ALWAYS advances the stability-fee accumulator
//! (§10.2); advances the redemption price (§10.3 / §6.4) only when its interval
//! is due and the oracle is fresh and non-zero, skipping rather than panicking
//! otherwise. A LEZ transaction carries exactly one instruction, so this is the
//! only way to advance both globals at once.
//!
//! The split between "always" and "best-effort" now lives in the apply halves:
//! this emits both effects unconditionally, and
//! [`try_advance_redemption`](crate::update_redemption_rate::try_advance_redemption)
//! returns `None` when a soft gate blocks. That is a better fit than v0.2.5's
//! version, which had to decide in the handler and emit an unchanged diff to
//! report a skip — the decision needs `last_updated_at`, which a plan cannot
//! read at all.

use lee_core::{account::AccountId, program::AccountMeta};
use stablecoin_core::ProtocolParameters;

/// Plan half: the union of the two standalone pokes' checks.
///
/// # Panics
/// - `caller` did not sign.
/// - any of the three globals is not at its canonical PDA.
/// - the oracle slot is not the bound handle, address or program.
/// - `clock` is not the system `CLOCK_01` shard.
#[expect(
    clippy::too_many_arguments,
    reason = "the account inputs are the union of the two standalone pokes' spec ABIs, plus the declared parameters"
)]
pub fn plan_refresh_globals(
    caller: &AccountMeta,
    protocol_parameters: &AccountMeta,
    stability_fee_accumulator: &AccountMeta,
    redemption_price_state: &AccountMeta,
    market_price_oracle: &AccountMeta,
    clock: &AccountMeta,
    parameters: &ProtocolParameters,
    stablecoin_program_id: AccountId,
) {
    assert!(caller.is_authorized, "Caller authorization is missing");
    crate::shared::assert_parameters_slot(protocol_parameters, stablecoin_program_id);
    crate::shared::assert_accumulator_slot(stability_fee_accumulator, stablecoin_program_id);
    crate::shared::assert_redemption_slot(redemption_price_state, stablecoin_program_id);
    crate::shared::assert_bound_oracle(market_price_oracle, parameters);
    crate::shared::assert_canonical_clock(clock);
}
