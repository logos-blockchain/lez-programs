use nssa_core::{
    account::{Account, AccountWithMetadata, Data},
    program::{AccountPostState, ChainedCall, ProgramId},
};
use stablecoin_core::{
    compute_stablecoin_definition_pda_seed, verify_position_and_get_seed, Position, FIXED_POINT_ONE,
};
use token_core::TokenHolding;
use twap_oracle_core::OraclePriceAccount;

use crate::shared::{
    read_clock_timestamp, read_protocol_parameters, read_redemption_price_state,
    read_stability_fee_accumulator,
};

/// Mints stablecoin debt against an existing position.
///
/// # Panics
/// Panics if the owner is not authorized, protocol accounts are invalid, the protocol is frozen,
/// the oracle is stale or malformed, the position is invalid, or the post-mint position would be
/// undercollateralized.
#[expect(
    clippy::too_many_arguments,
    reason = "instruction surface passes explicit position, token, oracle, and protocol accounts"
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
    let params = read_protocol_parameters(&protocol_parameters, stablecoin_program_id);
    assert!(
        !params.is_frozen,
        "Protocol is frozen; debt generation is disabled"
    );
    assert_eq!(
        stablecoin_definition.account_id, params.stablecoin_definition_id,
        "Stablecoin definition does not match protocol parameters"
    );

    let accumulator =
        read_stability_fee_accumulator(&stability_fee_accumulator, stablecoin_program_id);
    let redemption_state =
        read_redemption_price_state(&redemption_price_state, stablecoin_program_id);
    let now = read_clock_timestamp(&clock);
    let current_accumulator = stablecoin_core::current_accumulated_rate(&accumulator, &params, now);
    let current_redemption_price =
        stablecoin_core::current_redemption_price(&redemption_state, now);

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
    assert_eq!(
        position_data.owner_account_id, owner.account_id,
        "Position owner does not match signer"
    );
    let _position_seed = verify_position_and_get_seed(
        &position,
        &owner,
        position_data.position_nonce,
        stablecoin_program_id,
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
    assert_eq!(
        user_stablecoin_holding.account.program_owner, stablecoin_definition.account.program_owner,
        "Stablecoin holding and definition must be owned by the same Token Program"
    );
    let user_holding = TokenHolding::try_from(&user_stablecoin_holding.account.data)
        .expect("User stablecoin holding must hold a valid TokenHolding");
    assert_eq!(
        user_holding.definition_id(),
        stablecoin_definition.account_id,
        "Stablecoin holding does not match the provided stablecoin definition"
    );

    assert_eq!(
        market_price_oracle.account_id, params.market_price_oracle_id,
        "Market price oracle does not match protocol parameters"
    );
    assert_ne!(
        market_price_oracle.account,
        Account::default(),
        "Market price oracle account must be initialized"
    );
    let oracle = OraclePriceAccount::try_from(&market_price_oracle.account.data)
        .expect("Market price oracle account must hold a valid OraclePriceAccount");
    assert_eq!(
        oracle.base_asset, params.stablecoin_definition_id,
        "Market price oracle base asset must be the stablecoin definition"
    );
    assert_eq!(
        oracle.quote_asset, params.collateral_definition_id,
        "Market price oracle quote asset must be the collateral definition"
    );
    assert!(
        oracle.price != 0,
        "Market price oracle price must be non-zero"
    );
    assert!(
        now.saturating_sub(oracle.timestamp) <= params.maximum_oracle_price_age_milliseconds,
        "Market price oracle is stale"
    );

    let normalized_delta =
        stablecoin_core::mul_div_ceil(amount, FIXED_POINT_ONE, current_accumulator);
    let new_normalized_debt = position_data
        .normalized_debt_amount
        .checked_add(normalized_delta)
        .expect("Position normalized debt overflow");
    assert!(
        stablecoin_core::is_collateralized(
            position_data.collateral_amount,
            new_normalized_debt,
            current_accumulator,
            current_redemption_price,
            params.minimum_collateralization_ratio,
        ),
        "Position would be undercollateralized after debt generation"
    );

    let updated_position = Position {
        owner_account_id: position_data.owner_account_id,
        position_nonce: position_data.position_nonce,
        vault_account_id: position_data.vault_account_id,
        collateral_amount: position_data.collateral_amount,
        normalized_debt_amount: new_normalized_debt,
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
        AccountPostState::new(redemption_price_state.account),
        AccountPostState::new(market_price_oracle.account),
        AccountPostState::new(protocol_parameters.account),
        AccountPostState::new(clock.account),
    ];

    let mut stablecoin_definition_authorized = stablecoin_definition;
    stablecoin_definition_authorized.is_authorized = true;
    let mint_call = ChainedCall::new(
        stablecoin_definition_authorized.account.program_owner,
        vec![stablecoin_definition_authorized, user_stablecoin_holding],
        &token_core::Instruction::Mint {
            amount_to_mint: amount,
        },
    )
    .with_pda_seeds(vec![compute_stablecoin_definition_pda_seed()]);

    (post_states, vec![mint_call])
}
