use nssa_core::{
    account::{Account, AccountWithMetadata, Data},
    program::{AccountPostState, ChainedCall, Claim, ProgramId},
};
use stablecoin_core::{
    assert_protocol_parameter_bounds, compute_protocol_parameters_pda,
    compute_protocol_parameters_pda_seed, compute_redemption_price_state_pda,
    compute_redemption_price_state_pda_seed, compute_stability_fee_accumulator_pda,
    compute_stability_fee_accumulator_pda_seed, compute_stablecoin_definition_pda,
    compute_stablecoin_definition_pda_seed, compute_stablecoin_master_holding_pda,
    compute_stablecoin_master_holding_pda_seed, ProtocolParameters, RedemptionPriceState,
    StabilityFeeAccumulator, FIXED_POINT_ONE,
};
use token_core::TokenDefinition;
use twap_oracle_core::OraclePriceAccount;

use crate::shared::read_clock_timestamp;

/// Initializes stablecoin protocol globals and creates the stablecoin token definition.
///
/// # Panics
/// Panics if any account is not the expected PDA/state shape or any parameter is out of bounds.
#[expect(
    clippy::too_many_arguments,
    reason = "instruction surface initializes all protocol singleton accounts in one call"
)]
pub fn initialize_program(
    admin: AccountWithMetadata,
    protocol_parameters: AccountWithMetadata,
    stability_fee_accumulator: AccountWithMetadata,
    redemption_price_state: AccountWithMetadata,
    stablecoin_definition: AccountWithMetadata,
    stablecoin_master_holding: AccountWithMetadata,
    collateral_definition: AccountWithMetadata,
    market_price_oracle: AccountWithMetadata,
    clock: AccountWithMetadata,
    stablecoin_program_id: ProgramId,
    freeze_authority_account_id: nssa_core::account::AccountId,
    initial_stability_fee_per_millisecond: u128,
    initial_controller_proportional_gain: i128,
    initial_controller_integral_gain: i128,
    initial_minimum_collateralization_ratio: u128,
    minimum_milliseconds_between_rate_updates: u64,
    maximum_oracle_price_age_milliseconds: u64,
    initial_redemption_price: u128,
    stablecoin_name: String,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    assert!(admin.is_authorized, "Admin authorization is missing");
    assert_protocol_parameter_bounds(
        initial_stability_fee_per_millisecond,
        initial_controller_proportional_gain,
        initial_controller_integral_gain,
        initial_minimum_collateralization_ratio,
        minimum_milliseconds_between_rate_updates,
        maximum_oracle_price_age_milliseconds,
        initial_redemption_price,
    );

    assert_uninitialized_pda(
        &protocol_parameters,
        compute_protocol_parameters_pda(stablecoin_program_id),
        "Protocol parameters",
    );
    assert_uninitialized_pda(
        &stability_fee_accumulator,
        compute_stability_fee_accumulator_pda(stablecoin_program_id),
        "Stability fee accumulator",
    );
    assert_uninitialized_pda(
        &redemption_price_state,
        compute_redemption_price_state_pda(stablecoin_program_id),
        "Redemption price state",
    );
    assert_uninitialized_pda(
        &stablecoin_definition,
        compute_stablecoin_definition_pda(stablecoin_program_id),
        "Stablecoin definition",
    );
    assert_uninitialized_pda(
        &stablecoin_master_holding,
        compute_stablecoin_master_holding_pda(stablecoin_program_id),
        "Stablecoin master holding",
    );

    assert_ne!(
        collateral_definition.account,
        Account::default(),
        "Collateral definition account must be initialized"
    );
    let collateral_definition_data = TokenDefinition::try_from(&collateral_definition.account.data)
        .expect("Collateral definition account must hold a valid TokenDefinition");
    assert!(
        matches!(collateral_definition_data, TokenDefinition::Fungible { .. }),
        "Collateral definition must be fungible"
    );

    assert_ne!(
        market_price_oracle.account,
        Account::default(),
        "Market price oracle account must be initialized"
    );
    let oracle = OraclePriceAccount::try_from(&market_price_oracle.account.data)
        .expect("Market price oracle account must hold a valid OraclePriceAccount");
    assert_eq!(
        oracle.base_asset, stablecoin_definition.account_id,
        "Market price oracle base asset must be the stablecoin definition"
    );
    assert_eq!(
        oracle.quote_asset, collateral_definition.account_id,
        "Market price oracle quote asset must be the collateral definition"
    );

    let now = read_clock_timestamp(&clock);

    let protocol_data = ProtocolParameters {
        admin_account_id: admin.account_id,
        freeze_authority_account_id,
        stablecoin_definition_id: stablecoin_definition.account_id,
        collateral_definition_id: collateral_definition.account_id,
        market_price_oracle_id: market_price_oracle.account_id,
        stability_fee_per_millisecond: initial_stability_fee_per_millisecond,
        controller_proportional_gain: initial_controller_proportional_gain,
        controller_integral_gain: initial_controller_integral_gain,
        minimum_collateralization_ratio: initial_minimum_collateralization_ratio,
        minimum_milliseconds_between_rate_updates,
        maximum_oracle_price_age_milliseconds,
        is_frozen: false,
    };
    let mut protocol_post = protocol_parameters.account.clone();
    protocol_post.data = Data::from(&protocol_data);

    let mut fee_post = stability_fee_accumulator.account.clone();
    fee_post.data = Data::from(&StabilityFeeAccumulator {
        accumulated_rate_at_last_accrual: FIXED_POINT_ONE,
        last_accrued_at: now,
    });

    let mut redemption_post = redemption_price_state.account.clone();
    redemption_post.data = Data::from(&RedemptionPriceState {
        redemption_price_at_last_update: initial_redemption_price,
        redemption_rate_per_millisecond: FIXED_POINT_ONE,
        controller_integral_term: 0,
        last_updated_at: now,
    });

    let mut stablecoin_definition_authorized = stablecoin_definition.clone();
    stablecoin_definition_authorized.is_authorized = true;
    let mut stablecoin_master_holding_authorized = stablecoin_master_holding.clone();
    stablecoin_master_holding_authorized.is_authorized = true;
    let token_program_id = collateral_definition.account.program_owner;
    let create_stablecoin_call = ChainedCall::new(
        token_program_id,
        vec![
            stablecoin_definition_authorized,
            stablecoin_master_holding_authorized,
        ],
        &token_core::Instruction::NewFungibleDefinition {
            name: stablecoin_name,
            total_supply: 0,
            mint_authority: Some(stablecoin_definition.account_id),
        },
    )
    .with_pda_seeds(vec![
        compute_stablecoin_definition_pda_seed(),
        compute_stablecoin_master_holding_pda_seed(),
    ]);

    let post_states = vec![
        AccountPostState::new(admin.account),
        AccountPostState::new_claimed(
            protocol_post,
            Claim::Pda(compute_protocol_parameters_pda_seed()),
        ),
        AccountPostState::new_claimed(
            fee_post,
            Claim::Pda(compute_stability_fee_accumulator_pda_seed()),
        ),
        AccountPostState::new_claimed(
            redemption_post,
            Claim::Pda(compute_redemption_price_state_pda_seed()),
        ),
        AccountPostState::new(stablecoin_definition.account),
        AccountPostState::new(stablecoin_master_holding.account),
        AccountPostState::new(collateral_definition.account),
        AccountPostState::new(market_price_oracle.account),
        AccountPostState::new(clock.account),
    ];

    (post_states, vec![create_stablecoin_call])
}

fn assert_uninitialized_pda(
    account: &AccountWithMetadata,
    expected_id: nssa_core::account::AccountId,
    label: &str,
) {
    assert_eq!(
        account.account_id, expected_id,
        "{label} account ID does not match PDA"
    );
    assert_eq!(
        account.account,
        Account::default(),
        "{label} account must be uninitialized"
    );
}
