//! `Instruction::GenerateDebt` (spec §10.7).

use alloy_primitives::U512;
use lee_core::{
    account::{AccountId, ProgramShardSelector, ShardData},
    program::{AccountMeta, ChainedCall},
};
use stablecoin_core::{
    compute_stablecoin_definition_pda_seed,
    math::{mul_div_ceil, FIXED_POINT_ONE},
    CollateralizationInputs, GenerateDebt, Position, ProtocolParameters,
};

/// Plan half: check the addresses, gate on freshness and the freeze flag, and
/// chain the `Token::Mint`.
///
/// The oracle is a liveness gate only — §10.7 never consumes its price — and that
/// gate moves *into the plan*, which is where it now belongs: the observation is
/// declared, pinned exactly by its own guard, and `now` is pinned by the clock
/// guard, so comparing them needs no account data.
///
/// # Panics
/// - `owner` did not sign.
/// - any global is not at its canonical PDA, or the clock is not `CLOCK_01`.
/// - the protocol is frozen.
/// - `stablecoin_definition` is not the bound definition, or any token slot does not select the
///   bound Token Program's shard.
/// - the oracle is not the bound handle.
/// - the observation is dated in the future, or older than `maximum_oracle_price_age_milliseconds`.
/// - `position` is not at its `(owner, position_nonce)` PDA.
#[expect(
    clippy::too_many_arguments,
    reason = "the account inputs mirror the spec §10.7 ABI, plus the declared globals and clock"
)]
#[must_use]
pub fn plan_generate_debt(
    owner: &AccountMeta,
    position: &AccountMeta,
    stablecoin_definition: &AccountMeta,
    user_stablecoin_holding: &AccountMeta,
    stability_fee_accumulator: &AccountMeta,
    redemption_price_state: &AccountMeta,
    market_price_oracle: &AccountMeta,
    protocol_parameters: &AccountMeta,
    clock: &AccountMeta,
    parameters: &ProtocolParameters,
    oracle_timestamp: u64,
    now: u64,
    position_nonce: u64,
    amount: u128,
    stablecoin_program_id: AccountId,
) -> ChainedCall {
    assert!(owner.is_authorized, "Owner authorization is missing");
    crate::shared::assert_parameters_slot(protocol_parameters, stablecoin_program_id);
    crate::shared::assert_accumulator_slot(stability_fee_accumulator, stablecoin_program_id);
    crate::shared::assert_redemption_slot(redemption_price_state, stablecoin_program_id);
    crate::shared::assert_canonical_clock(clock);
    assert!(!parameters.is_frozen, "Protocol is frozen");
    assert_eq!(
        stablecoin_definition.account_id, parameters.stablecoin_definition_id,
        "Stablecoin definition does not match the one bound at initialize_program"
    );
    crate::shared::assert_token_program(stablecoin_definition, parameters, "Stablecoin definition");
    crate::shared::assert_token_program(
        user_stablecoin_holding,
        parameters,
        "User stablecoin holding",
    );
    crate::shared::assert_bound_oracle(market_price_oracle, parameters);
    assert_oracle_is_fresh(oracle_timestamp, now, parameters);
    crate::shared::assert_position_slot(position, owner, position_nonce, stablecoin_program_id);

    mint_call(
        parameters,
        stablecoin_definition.account_id,
        user_stablecoin_holding.account_id,
        amount,
    )
}

/// The oracle observation must be no older than the configured bound.
///
/// # Panics
/// If the observation is dated in the future, or is stale. A saturating
/// subtraction would report age 0 for a future-dated observation, reading as
/// maximally fresh and defeating the gate, so that case is rejected outright.
pub fn assert_oracle_is_fresh(oracle_timestamp: u64, now: u64, parameters: &ProtocolParameters) {
    let age = now
        .checked_sub(oracle_timestamp)
        .expect("Market price oracle observation is dated in the future");
    assert!(
        age <= parameters.maximum_oracle_price_age_milliseconds,
        "Market price oracle observation is stale"
    );
}

/// The chained `Token::Mint`, authorized by the stablecoin definition's PDA
/// seed — `initialize_program` made the definition its own mint authority.
#[must_use]
fn mint_call(
    parameters: &ProtocolParameters,
    stablecoin_definition_id: AccountId,
    user_stablecoin_holding_id: AccountId,
    amount: u128,
) -> ChainedCall {
    let token_program_id = parameters.token_program_id;
    ChainedCall::new(
        token_program_id,
        vec![
            ProgramShardSelector::new(stablecoin_definition_id, token_program_id),
            ProgramShardSelector::new(user_stablecoin_holding_id, token_program_id),
        ],
        &token_core::Instruction::Mint {
            amount_to_mint: amount,
        },
    )
    .with_pda_seeds(vec![compute_stablecoin_definition_pda_seed()])
}

/// Apply half: grow the position's normalized debt and re-check §6.2.
///
/// The delta is `⌈amount × FIXED_POINT_ONE / current_accumulator⌉`, rounded **up**
/// per §6.3: the borrower receives exactly `amount`, so nominal debt must grow by
/// at least `amount` and the remainder stays with the protocol.
///
/// Every input the check needs beyond the position lives in three other accounts
/// and arrives declared — see [`CollateralizationInputs`]. This apply is the one
/// place that sees all of them together, which is why the check lives here
/// rather than in the plan.
///
/// # Panics
/// - the shard is not a valid [`Position`], or its identity differs from the declared one.
/// - the debt addition overflows.
/// - the projected redemption price is zero. A rate below 1.0 decays it to zero over a long enough
///   gap, which would zero the required collateral and let any mint through.
/// - §6.2 fails after the mint.
#[must_use]
pub fn position_after_generate(pre_data: &ShardData, effect: &GenerateDebt) -> ShardData {
    let position = crate::shared::position_of(pre_data, &effect.expected);
    let current_accumulator = effect.inputs.fee.current_rate();
    let debt_delta = mul_div_ceil(effect.amount, FIXED_POINT_ONE, current_accumulator);
    let normalized_debt_amount = position
        .normalized_debt_amount
        .checked_add(debt_delta)
        .expect("Position normalized_debt_amount overflow");

    let updated = Position {
        normalized_debt_amount,
        ..position
    };
    assert_collateralized(&updated, &effect.inputs);
    ShardData::from(&updated)
}

/// The §6.2 invariant, against the accumulator and redemption price projected to
/// the pinned `now` (§5.3).
///
/// `current_accumulator` stays narrow — it divides `amount` above, so a debt delta
/// it cannot express is not a mint we can price — while the redemption price is
/// projected in `U512`, since a rate at the controller's permitted limit outgrows
/// `u128` about 45 minutes out and this value is only ever compared.
///
/// # Panics
/// If the projected redemption price is zero, or §6.2 fails.
pub fn assert_collateralized(position: &Position, inputs: &CollateralizationInputs) {
    let current_redemption_price = inputs.current_redemption_price_wide();
    assert!(
        !current_redemption_price.is_zero(),
        "Redemption price projected to zero"
    );
    crate::checks::assert_position_is_collateralized(
        position,
        U512::from(inputs.fee.current_rate()),
        current_redemption_price,
        inputs.minimum_collateralization_ratio,
    );
}
