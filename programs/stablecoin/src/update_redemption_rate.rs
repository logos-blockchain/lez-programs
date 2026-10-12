//! `Instruction::UpdateRedemptionRate` (spec §10.3) and the soft-gated half of
//! `RefreshGlobals` (§10.3a).

use lee_core::{
    account::{AccountId, ShardData},
    program::AccountMeta,
};
use stablecoin_core::{
    controller::run_controller_tick, math::compute_current_redemption_price, AdvanceRedemption,
    ControllerOutput, ProtocolParameters, RedemptionPriceState,
};

/// Plan half: check the caller, the two global addresses, and that the oracle
/// slot is the handle the protocol has bound.
///
/// Both halves of the oracle's identity are checked: its address *and* which
/// program's shard the slot selects. v0.2.5 deliberately left the producing
/// program unconstrained, and that intent survives — the program is bound once by
/// the admin rather than fixed in code — but it can no longer be chosen per call
/// by whoever submits the transaction. See
/// [`ProtocolParameters::market_price_oracle_program_id`].
///
/// # Panics
/// - `caller` did not sign.
/// - either global is not at its canonical PDA.
/// - the oracle slot is not the bound handle, address or program.
/// - `clock` is not the system `CLOCK_01` shard.
pub fn plan_update_redemption_rate(
    caller: &AccountMeta,
    protocol_parameters: &AccountMeta,
    redemption_price_state: &AccountMeta,
    market_price_oracle: &AccountMeta,
    clock: &AccountMeta,
    parameters: &ProtocolParameters,
    stablecoin_program_id: AccountId,
) {
    assert!(caller.is_authorized, "Caller authorization is missing");
    crate::shared::assert_parameters_slot(protocol_parameters, stablecoin_program_id);
    crate::shared::assert_redemption_slot(redemption_price_state, stablecoin_program_id);
    crate::shared::assert_bound_oracle(market_price_oracle, parameters);
    crate::shared::assert_canonical_clock(clock);
}

/// The [`AdvanceRedemption`] payload for a set of declared parameters, a declared
/// oracle observation and a declared clock.
///
/// Every field is copied from a value some other guard pins, so there is no
/// second input to lie about: the two timing bounds and the two gains come from
/// the parameters guard, the observation from the oracle guard, `now` from the
/// clock guard.
#[must_use]
pub fn advance_redemption_effect(
    parameters: &ProtocolParameters,
    oracle_price: u128,
    oracle_timestamp: u64,
    now: u64,
) -> AdvanceRedemption {
    AdvanceRedemption {
        oracle_price,
        oracle_timestamp,
        maximum_oracle_price_age_milliseconds: parameters.maximum_oracle_price_age_milliseconds,
        minimum_milliseconds_between_rate_updates: parameters
            .minimum_milliseconds_between_rate_updates,
        controller_proportional_gain: parameters.controller_proportional_gain,
        controller_integral_gain: parameters.controller_integral_gain,
        now,
    }
}

/// Why a controller tick cannot run, or `None` if it can.
///
/// The three conditions `update_redemption_rate` treats as failures and
/// `refresh_globals` treats as skips, in one place so the two cannot drift
/// apart. The interval check needs `last_updated_at`, which lives in the shard
/// being applied to — so this is evaluated here rather than in either plan.
fn blocked(state: &RedemptionPriceState, effect: &AdvanceRedemption) -> Option<&'static str> {
    if effect.now.saturating_sub(effect.oracle_timestamp)
        > effect.maximum_oracle_price_age_milliseconds
    {
        return Some("Market price oracle observation is stale");
    }
    if effect.oracle_price == 0 {
        return Some("Market price oracle reports zero price");
    }
    if effect.now.saturating_sub(state.last_updated_at)
        < effect.minimum_milliseconds_between_rate_updates
    {
        return Some("update_redemption_rate called too soon since last update");
    }
    None
}

/// Re-anchor the redemption price at `now` and run one controller tick.
fn ticked(state: &RedemptionPriceState, effect: &AdvanceRedemption) -> RedemptionPriceState {
    let milliseconds_elapsed = effect.now.saturating_sub(state.last_updated_at);
    let current_price = compute_current_redemption_price(
        state.redemption_price_at_last_update,
        state.redemption_rate_per_millisecond,
        state.last_updated_at,
        effect.now,
    );

    let ControllerOutput {
        redemption_rate_per_millisecond,
        controller_integral_term,
    } = run_controller_tick(
        current_price,
        effect.oracle_price,
        state.controller_integral_term,
        effect.controller_proportional_gain,
        effect.controller_integral_gain,
        milliseconds_elapsed,
    );

    RedemptionPriceState {
        redemption_price_at_last_update: current_price,
        redemption_rate_per_millisecond,
        controller_integral_term,
        last_updated_at: effect.now,
    }
}

/// Apply half of `UpdateRedemptionRate`: strict. A not-yet-due interval, a stale
/// oracle or a zero price is a failure (spec §10.3).
///
/// # Panics
/// If the shard is not a valid [`RedemptionPriceState`], or any gate blocks.
#[must_use]
pub fn advance_redemption(pre_data: &ShardData, effect: &AdvanceRedemption) -> ShardData {
    let state = RedemptionPriceState::try_from(pre_data)
        .expect("RedemptionPriceState account must be initialized and decode");
    if let Some(reason) = blocked(&state, effect) {
        panic!("{reason}");
    }
    ShardData::from(&ticked(&state, effect))
}

/// Apply half of `RefreshGlobals`' redemption half: best-effort. The same three
/// gates are *skips* here, not failures (spec §10.3a).
///
/// Returning `None` is what "skipped" now means. v0.2.5 had to emit an explicit
/// unchanged diff to say the same thing; an apply that declines to write says it
/// directly, and the asymmetry with [`advance_redemption`] is now visible in the
/// signature rather than in a branch.
///
/// # Panics
/// If the shard is not a valid [`RedemptionPriceState`].
#[must_use]
pub fn try_advance_redemption(
    pre_data: &ShardData,
    effect: &AdvanceRedemption,
) -> Option<ShardData> {
    let state = RedemptionPriceState::try_from(pre_data)
        .expect("RedemptionPriceState account must be initialized and decode");
    if blocked(&state, effect).is_some() {
        return None;
    }
    Some(ShardData::from(&ticked(&state, effect)))
}
