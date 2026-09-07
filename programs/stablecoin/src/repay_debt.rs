use lee_core::{
    account::{Account, AccountWithMetadata, Data},
    program::{AccountPostState, ChainedCall, ProgramId},
};
use stablecoin_core::{
    compute_protocol_parameters_pda, compute_stability_fee_accumulator_pda,
    math::{compute_current_accumulated_rate, mul_div, FIXED_POINT_ONE},
    verify_position_and_get_seed, Position, ProtocolParameters, StabilityFeeAccumulator,
};
use token_core::TokenHolding;

/// Repay `amount` of outstanding stablecoin debt against an existing position.
///
/// Burns `amount` stablecoins from `user_stablecoin_holding` via a chained
/// `Token::Burn` and decreases `Position.normalized_debt_amount` by the same
/// amount. The position post-state uses plain [`AccountPostState::new`] — the
/// PDA was already claimed at `open_position` time.
///
/// The normalized-debt decrement is `⌊amount × FIXED_POINT_ONE /
/// current_accumulator⌋`, rounded **down** per §6.3, so the position's debt
/// shrinks by at most what was burned and the rounding remainder stays with the
/// protocol. The accumulator is projected forward to the clock timestamp (§5.3).
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
/// - The floored decrement exceeds `Position.normalized_debt_amount`.
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
    stablecoin_program_id: ProgramId,
    amount: u128,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
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

    // Round DOWN (§6.3): the borrower burned exactly `amount`, and their debt
    // shrinks by at most that much. The remainder is fee credit for the protocol.
    let debt_delta = mul_div(amount, FIXED_POINT_ONE, current_accumulator);
    let new_debt = position_data
        .normalized_debt_amount
        .checked_sub(debt_delta)
        .expect("Repay amount exceeds outstanding debt");

    let updated_position = Position {
        owner_account_id: position_data.owner_account_id,
        position_nonce: position_data.position_nonce,
        vault_account_id: position_data.vault_account_id,
        collateral_amount: position_data.collateral_amount,
        normalized_debt_amount: new_debt,
        opened_at: position_data.opened_at,
    };
    let mut position_post = position.account.clone();
    position_post.data = Data::from(&updated_position);

    let post_states = vec![
        AccountPostState::new(owner.account),
        AccountPostState::new(position_post),
        AccountPostState::new(stablecoin_definition.account.clone()),
        AccountPostState::new(user_stablecoin_holding.account.clone()),
        AccountPostState::new(stability_fee_accumulator.account),
        AccountPostState::new(protocol_parameters.account),
        AccountPostState::new(clock.account),
    ];

    let token_program_id = user_stablecoin_holding.account.program_owner;
    let burn_call = ChainedCall::new(
        token_program_id,
        vec![stablecoin_definition, user_stablecoin_holding],
        &token_core::Instruction::Burn {
            amount_to_burn: amount,
        },
    );

    (post_states, vec![burn_call])
}
