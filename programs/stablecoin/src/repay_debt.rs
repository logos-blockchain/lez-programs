use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, BalanceDiff, Data},
    program::{AccountStateDiff, ChainedCall},
};
use stablecoin_core::{
    compute_protocol_parameters_pda, compute_stability_fee_accumulator_pda,
    math::{compute_current_accumulated_rate, compute_repayment},
    verify_position_and_get_seed, Position, ProtocolParameters, StabilityFeeAccumulator,
};
use token_core::TokenHolding;

/// Repay outstanding debt with `amount` as the maximum authorized stablecoin burn.
///
/// Projects the accumulator to the clock timestamp and caps the chained
/// `Token::Burn` at the ceiling of the position's current debt. A sufficient
/// maximum clears normalized debt; the unused buffer stays in the user's
/// holding. Otherwise the entire maximum is burned and the normalized decrement
/// is rounded down: `⌊amount × FIXED_POINT_ONE / current_accumulator⌋` (§6.3).
/// The position diff is a plain data write; its PDA was claimed at opening.
/// Zero maxima and debt-free positions still validate all accounts and emit a
/// zero-amount burn.
///
/// Allowed while the protocol is frozen — repaying only improves the protocol's
/// position (§7). `stablecoin_definition` is pinned against
/// `ProtocolParameters.stablecoin_definition_id`.
///
/// # Panics
/// - `owner` is not authorized.
/// - `position` is uninitialized, not owned by `stablecoin_program_id`, holds data that does not
///   decode as a [`Position`], or sits at an address that does not match
///   `compute_position_pda(stablecoin_program_id, owner, Position.position_nonce)`.
/// - `user_stablecoin_holding` is not authorized, is uninitialized, is owned by a different Token
///   Program than `stablecoin_definition`, or holds a [`TokenHolding`] whose `definition_id` does
///   not match `stablecoin_definition.account_id`.
/// - `stablecoin_definition` is uninitialized, or does not match
///   `protocol_parameters.stablecoin_definition_id`.
/// - `protocol_parameters` or `stability_fee_accumulator` is uninitialized, wrongly owned, not at
///   its canonical PDA, or does not decode.
/// - `clock` is not the initialized system `CLOCK_01` account.
/// - Projecting the accumulator exceeds the supported arithmetic range.
#[allow(
    clippy::too_many_arguments,
    reason = "the seven account inputs mirror the spec §10.8 ABI"
)]
pub fn repay_debt(
    owner: AccountWithMetadata,
    position: AccountWithMetadata,
    stablecoin_definition: AccountWithMetadata,
    user_stablecoin_holding: AccountWithMetadata,
    stability_fee_accumulator: AccountWithMetadata,
    protocol_parameters: AccountWithMetadata,
    clock: AccountWithMetadata,
    stablecoin_program_id: AccountId,
    amount: u128,
) -> (Vec<AccountStateDiff>, Vec<ChainedCall>) {
    assert!(owner.is_authorized, "Owner authorization is missing");
    assert_ne!(
        position.account,
        Account::default(),
        "Position account must be initialized"
    );
    assert_eq!(
        position.account.program_owner, stablecoin_program_id,
        "Position is not owned by this stablecoin program"
    );

    let position_data = Position::try_from(&position.account.data)
        .expect("Position account must hold valid Position state");
    // `verify_position_and_get_seed` asserts the position address matches the
    // (owner, position_nonce) PDA derivation. The returned seed is
    // dropped — the position is already PDA-claimed.
    let _position_seed = verify_position_and_get_seed(
        &position,
        &owner,
        position_data.position_nonce,
        stablecoin_program_id,
    );
    // The PDA derivation above already binds the owner; this guards the stored
    // discovery copy against silently drifting out of sync.
    assert_eq!(
        position_data.owner_account_id, owner.account_id,
        "Position owner_account_id does not match the owner account"
    );

    assert!(
        user_stablecoin_holding.is_authorized,
        "User stablecoin holding authorization is missing"
    );

    let parameters = ProtocolParameters::try_from(&crate::checks::decode_global(
        &protocol_parameters,
        compute_protocol_parameters_pda(stablecoin_program_id),
        stablecoin_program_id,
        "ProtocolParameters",
    ))
    .expect("ProtocolParameters must decode");
    // `is_frozen` is deliberately not read: repaying only improves the protocol's
    // position, so spec §7 keeps it available while frozen.
    assert_eq!(
        stablecoin_definition.account_id, parameters.stablecoin_definition_id,
        "Stablecoin definition does not match the one bound at initialize_program"
    );

    assert_ne!(
        user_stablecoin_holding.account,
        Account::default(),
        "User stablecoin holding must be initialized"
    );
    assert_ne!(
        stablecoin_definition.account,
        Account::default(),
        "Stablecoin definition account must be initialized"
    );
    assert_eq!(
        user_stablecoin_holding.account.program_owner, stablecoin_definition.account.program_owner,
        "Stablecoin holding and definition must be owned by the same Token Program"
    );
    let user_holding_data = TokenHolding::try_from(&user_stablecoin_holding.account.data)
        .expect("User stablecoin holding must hold a valid TokenHolding");
    assert_eq!(
        user_holding_data.definition_id(),
        stablecoin_definition.account_id,
        "Stablecoin holding does not match the provided stablecoin definition"
    );

    let accumulator = StabilityFeeAccumulator::try_from(&crate::checks::decode_global(
        &stability_fee_accumulator,
        compute_stability_fee_accumulator_pda(stablecoin_program_id),
        stablecoin_program_id,
        "StabilityFeeAccumulator",
    ))
    .expect("StabilityFeeAccumulator must decode");

    let now = crate::accrue_stability_fee::read_clock(&clock);
    let current_accumulator = compute_current_accumulated_rate(
        accumulator.accumulated_rate_at_last_accrual,
        parameters.stability_fee_per_millisecond,
        accumulator.last_accrued_at,
        now,
    );

    let repayment = compute_repayment(
        amount,
        position_data.normalized_debt_amount,
        current_accumulator,
    );
    let new_debt = position_data
        .normalized_debt_amount
        .checked_sub(repayment.normalized_debt_decrement)
        .expect("Repayment decrement is capped at outstanding normalized debt");

    let updated_position = Position {
        owner_account_id: position_data.owner_account_id,
        position_nonce: position_data.position_nonce,
        vault_account_id: position_data.vault_account_id,
        collateral_amount: position_data.collateral_amount,
        normalized_debt_amount: new_debt,
        opened_at: position_data.opened_at,
    };
    let token_program_id = user_stablecoin_holding.account.program_owner;
    let stablecoin_definition_id = stablecoin_definition.account_id;
    let user_stablecoin_holding_id = user_stablecoin_holding.account_id;

    let state_diffs = vec![
        AccountStateDiff::unchanged(owner),
        AccountStateDiff::new(position, BalanceDiff::Add(0), Data::from(&updated_position)),
        AccountStateDiff::unchanged(stablecoin_definition),
        AccountStateDiff::unchanged(user_stablecoin_holding),
        AccountStateDiff::unchanged(stability_fee_accumulator),
        AccountStateDiff::unchanged(protocol_parameters),
        AccountStateDiff::unchanged(clock),
    ];

    let burn_call = ChainedCall::new(
        token_program_id,
        vec![stablecoin_definition_id, user_stablecoin_holding_id],
        &token_core::Instruction::Burn {
            amount_to_burn: repayment.amount_to_burn,
        },
    );

    (state_diffs, vec![burn_call])
}
