use lee_core::{
    account::{Account, AccountWithMetadata, Data},
    program::{AccountPostState, ChainedCall, ProgramId},
};
use stablecoin_core::{
    compute_protocol_parameters_pda, compute_redemption_price_state_pda,
    compute_stability_fee_accumulator_pda, compute_stablecoin_definition_pda_seed,
    math::{
        compute_current_accumulated_rate, compute_current_redemption_price, mul_div_ceil,
        FIXED_POINT_ONE,
    },
    verify_position_and_get_seed, Position, ProtocolParameters, RedemptionPriceState,
    StabilityFeeAccumulator,
};
use token_core::TokenHolding;

/// Mint `amount` stablecoins against `position`, increasing its debt (spec §10.7).
///
/// Emits a chained `Token::Mint` authorized by the stablecoin definition's PDA
/// seed — `initialize_program` set the definition as its own mint authority.
/// The position's `normalized_debt_amount` grows by
/// `⌈amount × FIXED_POINT_ONE / current_accumulator⌉`, rounded **up** per §6.3 so
/// the borrower's nominal debt grows by at least `amount`.
///
/// The §6.2 collateralization invariant is checked *after* the mint, against the
/// accumulator and redemption price projected forward to the clock timestamp.
/// The oracle is read for its staleness gate only; its price is not used.
///
/// # Panics
/// - `owner` is not authorized.
/// - `position` is uninitialized, not owned by `stablecoin_program_id`, does not decode, or is not
///   at its `(owner, position_nonce)` PDA.
/// - `protocol_parameters`, `stability_fee_accumulator` or `redemption_price_state` is
///   uninitialized, wrongly owned, not at its canonical PDA, or does not decode.
/// - `protocol_parameters.is_frozen` is set.
/// - `stablecoin_definition.account_id` does not match
///   `protocol_parameters.stablecoin_definition_id`, or it is uninitialized.
/// - `market_price_oracle` does not match `protocol_parameters.market_price_oracle_id`, or its
///   observation is older than `maximum_oracle_price_age_milliseconds`.
/// - `user_stablecoin_holding` is uninitialized, owned by a different Token Program than the
///   definition, or holds a different definition.
/// - `clock` is not the initialized system `CLOCK_01` account.
/// - The debt addition overflows, or §6.2 fails post-mint.
#[allow(
    clippy::too_many_arguments,
    reason = "the nine account inputs mirror the spec §10.7 ABI"
)]
pub fn generate_debt(
    owner: AccountWithMetadata,
    position: AccountWithMetadata,
    stablecoin_definition: AccountWithMetadata,
    user_stablecoin_holding: AccountWithMetadata,
    stability_fee_accumulator: AccountWithMetadata,
    redemption_price_state: AccountWithMetadata,
    market_price_oracle: AccountWithMetadata,
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
    let _position_seed = verify_position_and_get_seed(
        &position,
        &owner,
        position_data.position_nonce,
        stablecoin_program_id,
    );
    assert_eq!(
        position_data.owner_account_id, owner.account_id,
        "Position owner_account_id does not match the owner account"
    );

    let parameters = ProtocolParameters::try_from(&crate::checks::decode_global(
        &protocol_parameters,
        compute_protocol_parameters_pda(stablecoin_program_id),
        stablecoin_program_id,
        "ProtocolParameters",
    ))
    .expect("ProtocolParameters must decode");
    assert!(!parameters.is_frozen, "Protocol is frozen");

    let accumulator = StabilityFeeAccumulator::try_from(&crate::checks::decode_global(
        &stability_fee_accumulator,
        compute_stability_fee_accumulator_pda(stablecoin_program_id),
        stablecoin_program_id,
        "StabilityFeeAccumulator",
    ))
    .expect("StabilityFeeAccumulator must decode");

    let redemption = RedemptionPriceState::try_from(&crate::checks::decode_global(
        &redemption_price_state,
        compute_redemption_price_state_pda(stablecoin_program_id),
        stablecoin_program_id,
        "RedemptionPriceState",
    ))
    .expect("RedemptionPriceState must decode");

    let now = crate::accrue_stability_fee::read_clock(&clock);

    // The oracle is a liveness gate only — spec §10.7 never consumes its price.
    let oracle = crate::update_redemption_rate::decode_oracle(&market_price_oracle, &parameters);
    assert!(
        now.saturating_sub(oracle.timestamp) <= parameters.maximum_oracle_price_age_milliseconds,
        "Market price oracle observation is stale"
    );

    assert_eq!(
        stablecoin_definition.account_id, parameters.stablecoin_definition_id,
        "Stablecoin definition does not match the one bound at initialize_program"
    );
    assert_ne!(
        stablecoin_definition.account,
        Account::default(),
        "Stablecoin definition account must be initialized"
    );

    assert_ne!(
        user_stablecoin_holding.account,
        Account::default(),
        "User stablecoin holding must be initialized"
    );
    let token_program_id = stablecoin_definition.account.program_owner;
    assert_eq!(
        user_stablecoin_holding.account.program_owner, token_program_id,
        "User stablecoin holding must be owned by the same Token Program as the definition"
    );
    let user_holding = TokenHolding::try_from(&user_stablecoin_holding.account.data)
        .expect("User stablecoin holding must hold a valid TokenHolding");
    assert_eq!(
        user_holding.definition_id(),
        stablecoin_definition.account_id,
        "User stablecoin holding does not match the stablecoin definition"
    );

    let current_accumulator = compute_current_accumulated_rate(
        accumulator.accumulated_rate_at_last_accrual,
        parameters.stability_fee_per_millisecond,
        accumulator.last_accrued_at,
        now,
    );
    // Round UP (§6.3): the borrower receives exactly `amount`, so nominal debt
    // must grow by at least `amount`. The remainder stays with the protocol.
    let debt_delta = mul_div_ceil(amount, FIXED_POINT_ONE, current_accumulator);
    let new_debt = position_data
        .normalized_debt_amount
        .checked_add(debt_delta)
        .expect("Position normalized_debt_amount overflow");

    let updated_position = Position {
        normalized_debt_amount: new_debt,
        ..position_data
    };
    crate::checks::assert_position_is_collateralized(
        &updated_position,
        current_accumulator,
        compute_current_redemption_price(
            redemption.redemption_price_at_last_update,
            redemption.redemption_rate_per_millisecond,
            redemption.last_updated_at,
            now,
        ),
        parameters.minimum_collateralization_ratio,
    );

    let mut position_post = position.account.clone();
    position_post.data = Data::from(&updated_position);

    let post_states = vec![
        AccountPostState::new(owner.account),
        AccountPostState::new(position_post),
        AccountPostState::new(stablecoin_definition.account.clone()),
        AccountPostState::new(user_stablecoin_holding.account.clone()),
        AccountPostState::new(stability_fee_accumulator.account),
        AccountPostState::new(redemption_price_state.account),
        AccountPostState::new(market_price_oracle.account),
        AccountPostState::new(protocol_parameters.account),
        AccountPostState::new(clock.account),
    ];

    // `initialize_program` made the definition its own mint authority, so the
    // chained call authorizes it with the definition's PDA seed.
    let mut definition_authorized = stablecoin_definition.clone();
    definition_authorized.is_authorized = true;
    let mint_call = ChainedCall::new(
        token_program_id,
        vec![definition_authorized, user_stablecoin_holding],
        &token_core::Instruction::Mint {
            amount_to_mint: amount,
        },
    )
    .with_pda_seeds(vec![compute_stablecoin_definition_pda_seed()]);

    (post_states, vec![mint_call])
}
