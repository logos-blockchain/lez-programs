//! Host-side implementation of `Instruction::InitializeProgram` (spec §10.1).
//!
//! Wall-clock time is read from the system `CLOCK_01` account passed as the
//! 9th input. The pinned `spel-framework` (v0.3.0) `ProgramContext` exposes no
//! clock, so — like the runtime's own time-locked programs — this reads the
//! millisecond timestamp from a clock account (see `clock_core`).

use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use nssa_core::{
    account::{Account, AccountId, AccountWithMetadata, Data},
    program::{AccountPostState, ChainedCall, Claim, ProgramId},
};
use stablecoin_core::{
    compute_protocol_parameters_pda, compute_protocol_parameters_pda_seed,
    compute_redemption_price_state_pda, compute_redemption_price_state_pda_seed,
    compute_stability_fee_accumulator_pda, compute_stability_fee_accumulator_pda_seed,
    compute_stablecoin_definition_pda, compute_stablecoin_definition_pda_seed,
    compute_stablecoin_master_holding_pda, compute_stablecoin_master_holding_pda_seed,
    math::FIXED_POINT_ONE, ProtocolParameters, RedemptionPriceState, StabilityFeeAccumulator,
};
use token_core::TokenDefinition;
use twap_oracle_core::OraclePriceAccount;

// --- Sane-band constants per spec §8 -----------------------------------------

const MAX_STABILITY_FEE_PER_MILLISECOND: u128 = FIXED_POINT_ONE * 2;
const MIN_COLLATERALIZATION_RATIO: u128 = FIXED_POINT_ONE * 110 / 100; // 1.1x
const MAX_COLLATERALIZATION_RATIO: u128 = FIXED_POINT_ONE * 10;
// Gain magnitude caps (spec §8; placeholders pending the §15 tuning pass):
// |Kp| <= FIXED_POINT_ONE * 10^3, |Ki| <= FIXED_POINT_ONE.
const MAX_PROPORTIONAL_GAIN_MAGNITUDE: u128 = FIXED_POINT_ONE * 1_000;
const MAX_INTEGRAL_GAIN_MAGNITUDE: u128 = FIXED_POINT_ONE;
const MAX_TIMING_MILLISECONDS: u64 = 86_400_000; // 1 day

// --- Unpacked numerical parameters -------------------------------------------

/// Numerical parameters for [`initialize_program`]. Bundled to keep the host
/// function's argument count manageable; this is a parameter bag, not state.
#[derive(Debug)]
pub struct InitializeProgramParams<'a> {
    pub freeze_authority_account_id: AccountId,
    pub initial_stability_fee_per_millisecond: u128,
    pub initial_controller_proportional_gain: i128,
    pub initial_controller_integral_gain: i128,
    pub initial_minimum_collateralization_ratio: u128,
    pub minimum_milliseconds_between_rate_updates: u64,
    pub maximum_oracle_price_age_milliseconds: u64,
    pub initial_redemption_price: u128,
    pub stablecoin_name: &'a str,
}

// --- Host function ----------------------------------------------------------

/// Bootstrap a fresh stablecoin protocol instance.
///
/// See spec §10.1 for the full account contract and panic conditions. `now` is
/// read from the `clock` input (the system `CLOCK_01` account); it anchors the
/// stability-fee accumulator and the redemption-price state.
#[allow(
    clippy::too_many_arguments,
    reason = "the nine account inputs + program id + parameter bag mirror the on-chain instruction ABI; collapsing them further would obscure it"
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
    params: InitializeProgramParams<'_>,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    // 1. Authorization
    assert!(admin.is_authorized, "Admin authorization is missing");

    // 2. Target PDAs must be uninitialized
    assert_eq!(
        protocol_parameters.account,
        Account::default(),
        "ProtocolParameters account must be uninitialized"
    );
    assert_eq!(
        stability_fee_accumulator.account,
        Account::default(),
        "StabilityFeeAccumulator account must be uninitialized"
    );
    assert_eq!(
        redemption_price_state.account,
        Account::default(),
        "RedemptionPriceState account must be uninitialized"
    );
    assert_eq!(
        stablecoin_definition.account,
        Account::default(),
        "StablecoinDefinition account must be uninitialized"
    );
    assert_eq!(
        stablecoin_master_holding.account,
        Account::default(),
        "StablecoinMasterHolding account must be uninitialized"
    );

    // 3. PDA address checks
    assert_eq!(
        protocol_parameters.account_id,
        compute_protocol_parameters_pda(stablecoin_program_id),
        "ProtocolParameters account ID does not match expected PDA derivation"
    );
    assert_eq!(
        stability_fee_accumulator.account_id,
        compute_stability_fee_accumulator_pda(stablecoin_program_id),
        "StabilityFeeAccumulator account ID does not match expected PDA derivation"
    );
    assert_eq!(
        redemption_price_state.account_id,
        compute_redemption_price_state_pda(stablecoin_program_id),
        "RedemptionPriceState account ID does not match expected PDA derivation"
    );
    assert_eq!(
        stablecoin_definition.account_id,
        compute_stablecoin_definition_pda(stablecoin_program_id),
        "StablecoinDefinition account ID does not match expected PDA derivation"
    );
    assert_eq!(
        stablecoin_master_holding.account_id,
        compute_stablecoin_master_holding_pda(stablecoin_program_id),
        "StablecoinMasterHolding account ID does not match expected PDA derivation"
    );

    // 4. Clock account: read the millisecond wall-clock timestamp.
    assert_eq!(
        clock.account_id, CLOCK_01_PROGRAM_ACCOUNT_ID,
        "Clock account must be the system CLOCK_01 account"
    );
    assert_ne!(
        clock.account,
        Account::default(),
        "Clock account must be initialized"
    );
    let now = ClockAccountData::from_bytes(clock.account.data.as_ref()).timestamp;

    // 5. Collateral definition must be an initialized Fungible TokenDefinition
    assert_ne!(
        collateral_definition.account,
        Account::default(),
        "Collateral definition account must be initialized"
    );
    let collateral_def = TokenDefinition::try_from(&collateral_definition.account.data)
        .expect("Collateral definition must be a valid TokenDefinition");
    assert!(
        matches!(collateral_def, TokenDefinition::Fungible { .. }),
        "Collateral definition must be Fungible"
    );

    // 6. Market price oracle must be initialized + base/quote match
    assert_ne!(
        market_price_oracle.account,
        Account::default(),
        "Market price oracle account must be initialized"
    );
    let oracle = OraclePriceAccount::try_from(&market_price_oracle.account.data)
        .expect("Market price oracle must decode as OraclePriceAccount");
    assert_eq!(
        oracle.base_asset, stablecoin_definition.account_id,
        "Oracle base_asset must equal the stablecoin definition's account_id"
    );
    assert_eq!(
        oracle.quote_asset, collateral_definition.account_id,
        "Oracle quote_asset must equal the collateral definition's account_id"
    );

    // 7. Numerical param bounds (spec §8)
    assert!(
        params.initial_stability_fee_per_millisecond >= FIXED_POINT_ONE,
        "initial_stability_fee_per_millisecond below FIXED_POINT_ONE"
    );
    assert!(
        params.initial_stability_fee_per_millisecond <= MAX_STABILITY_FEE_PER_MILLISECOND,
        "initial_stability_fee_per_millisecond above sane upper bound"
    );
    assert!(
        params.initial_minimum_collateralization_ratio >= MIN_COLLATERALIZATION_RATIO,
        "initial_minimum_collateralization_ratio below 1.1x"
    );
    assert!(
        params.initial_minimum_collateralization_ratio <= MAX_COLLATERALIZATION_RATIO,
        "initial_minimum_collateralization_ratio above 10x"
    );
    assert!(
        params.initial_controller_proportional_gain.unsigned_abs()
            <= MAX_PROPORTIONAL_GAIN_MAGNITUDE,
        "controller_proportional_gain out of band"
    );
    assert!(
        params.initial_controller_integral_gain.unsigned_abs() <= MAX_INTEGRAL_GAIN_MAGNITUDE,
        "controller_integral_gain out of band"
    );
    for &(milliseconds, label) in &[
        (
            params.minimum_milliseconds_between_rate_updates,
            "minimum_milliseconds_between_rate_updates",
        ),
        (
            params.maximum_oracle_price_age_milliseconds,
            "maximum_oracle_price_age_milliseconds",
        ),
    ] {
        assert!(milliseconds >= 1, "{label} below minimum 1ms");
        assert!(
            milliseconds <= MAX_TIMING_MILLISECONDS,
            "{label} above maximum 86_400_000ms"
        );
    }
    assert!(
        params.initial_redemption_price > 0,
        "initial_redemption_price must be positive"
    );
    assert!(
        !params.stablecoin_name.is_empty(),
        "stablecoin_name must be non-empty"
    );

    // 8. Build the new global states
    let protocol_parameters_value = ProtocolParameters {
        admin_account_id: admin.account_id,
        freeze_authority_account_id: params.freeze_authority_account_id,
        stablecoin_definition_id: stablecoin_definition.account_id,
        collateral_definition_id: collateral_definition.account_id,
        market_price_oracle_id: market_price_oracle.account_id,
        stability_fee_per_millisecond: params.initial_stability_fee_per_millisecond,
        controller_proportional_gain: params.initial_controller_proportional_gain,
        controller_integral_gain: params.initial_controller_integral_gain,
        minimum_collateralization_ratio: params.initial_minimum_collateralization_ratio,
        minimum_milliseconds_between_rate_updates: params.minimum_milliseconds_between_rate_updates,
        maximum_oracle_price_age_milliseconds: params.maximum_oracle_price_age_milliseconds,
        is_frozen: false,
    };
    let accumulator_value = StabilityFeeAccumulator {
        accumulated_rate_at_last_accrual: FIXED_POINT_ONE,
        last_accrued_at: now,
    };
    let redemption_value = RedemptionPriceState {
        redemption_price_at_last_update: params.initial_redemption_price,
        redemption_rate_per_millisecond: FIXED_POINT_ONE,
        controller_integral_term: 0,
        last_updated_at: now,
    };

    // 9. Post-states (9 accounts)
    let mut protocol_parameters_post = protocol_parameters.account;
    protocol_parameters_post.data = Data::from(&protocol_parameters_value);

    let mut accumulator_post = stability_fee_accumulator.account;
    accumulator_post.data = Data::from(&accumulator_value);

    let mut redemption_post = redemption_price_state.account;
    redemption_post.data = Data::from(&redemption_value);

    let protocol_seed = compute_protocol_parameters_pda_seed();
    let accumulator_seed = compute_stability_fee_accumulator_pda_seed();
    let redemption_seed = compute_redemption_price_state_pda_seed();
    let stablecoin_def_seed = compute_stablecoin_definition_pda_seed();
    let master_holding_seed = compute_stablecoin_master_holding_pda_seed();

    let token_program_id = collateral_definition.account.program_owner;

    // For the chained Token::NewFungibleDefinition we mark both target PDAs
    // authorized — the chained call's PDA seeds authorize the claim.
    let mut stablecoin_def_authorized = stablecoin_definition.clone();
    stablecoin_def_authorized.is_authorized = true;
    let mut master_holding_authorized = stablecoin_master_holding.clone();
    master_holding_authorized.is_authorized = true;

    let post_states = vec![
        AccountPostState::new(admin.account),
        AccountPostState::new_claimed(protocol_parameters_post, Claim::Pda(protocol_seed)),
        AccountPostState::new_claimed(accumulator_post, Claim::Pda(accumulator_seed)),
        AccountPostState::new_claimed(redemption_post, Claim::Pda(redemption_seed)),
        AccountPostState::new(stablecoin_definition.account.clone()),
        AccountPostState::new(stablecoin_master_holding.account.clone()),
        AccountPostState::new(collateral_definition.account.clone()),
        AccountPostState::new(market_price_oracle.account.clone()),
        AccountPostState::new(clock.account.clone()),
    ];

    let new_definition_call = ChainedCall::new(
        token_program_id,
        vec![stablecoin_def_authorized, master_holding_authorized],
        &token_core::Instruction::NewFungibleDefinition {
            name: params.stablecoin_name.to_owned(),
            total_supply: 0,
            // Self/PDA authority: the definition account is its own mint authority, so
            // `generate_debt` / `repay_debt` can mint and burn by presenting the
            // stablecoin definition PDA seed in their chained Token calls.
            mint_authority: Some(stablecoin_definition.account_id),
        },
    )
    .with_pda_seeds(vec![stablecoin_def_seed, master_holding_seed]);

    (post_states, vec![new_definition_call])
}

#[cfg(test)]
#[allow(
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    reason = "tests deliberately panic on bad state via assert!/#[should_panic] and index fixed-size vectors"
)]
mod tests {
    use nssa_core::account::Nonce;
    use stablecoin_core::compute_protocol_parameters_pda_seed;

    use super::*;

    const STABLECOIN_PROGRAM_ID: ProgramId = [3u32; 8];
    const TOKEN_PROGRAM_ID: ProgramId = [2u32; 8];
    const ORACLE_PROGRAM_ID: ProgramId = [4u32; 8];
    const CLOCK_PROGRAM_ID: ProgramId = [5u32; 8];
    const NOW: u64 = 1_700_000_000;

    fn admin_id() -> AccountId {
        AccountId::new([0xA0; 32])
    }
    fn freeze_id() -> AccountId {
        AccountId::new([0xFE; 32])
    }
    fn collateral_definition_id() -> AccountId {
        AccountId::new([0x10; 32])
    }
    fn oracle_id() -> AccountId {
        AccountId::new([0x20; 32])
    }
    fn oracle_source_id() -> AccountId {
        AccountId::new([0x21; 32])
    }

    fn protocol_parameters_id() -> AccountId {
        compute_protocol_parameters_pda(STABLECOIN_PROGRAM_ID)
    }
    fn accumulator_id() -> AccountId {
        compute_stability_fee_accumulator_pda(STABLECOIN_PROGRAM_ID)
    }
    fn redemption_id() -> AccountId {
        compute_redemption_price_state_pda(STABLECOIN_PROGRAM_ID)
    }
    fn stablecoin_def_id() -> AccountId {
        compute_stablecoin_definition_pda(STABLECOIN_PROGRAM_ID)
    }
    fn master_holding_id() -> AccountId {
        compute_stablecoin_master_holding_pda(STABLECOIN_PROGRAM_ID)
    }

    fn admin_account() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account::default(),
            is_authorized: true,
            account_id: admin_id(),
        }
    }

    fn uninit(account_id: AccountId) -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account::default(),
            is_authorized: false,
            account_id,
        }
    }

    fn collateral_definition_account() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: TOKEN_PROGRAM_ID,
                balance: 0,
                data: Data::from(&TokenDefinition::Fungible {
                    name: "SNT".to_owned(),
                    total_supply: 1_000_000,
                    metadata_id: None,
                    authority: None,
                }),
                nonce: Nonce(0),
            },
            is_authorized: false,
            account_id: collateral_definition_id(),
        }
    }

    fn oracle_account() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: ORACLE_PROGRAM_ID,
                balance: 0,
                data: Data::from(&OraclePriceAccount {
                    base_asset: stablecoin_def_id(),
                    quote_asset: collateral_definition_id(),
                    price: FIXED_POINT_ONE / 2,
                    timestamp: NOW,
                    source_id: oracle_source_id(),
                    confidence_interval: 0,
                }),
                nonce: Nonce(0),
            },
            is_authorized: false,
            account_id: oracle_id(),
        }
    }

    fn clock_account() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: CLOCK_PROGRAM_ID,
                balance: 0,
                data: Data::try_from(
                    ClockAccountData {
                        block_id: 0,
                        timestamp: NOW,
                    }
                    .to_bytes(),
                )
                .expect("clock data fits"),
                nonce: Nonce(0),
            },
            is_authorized: false,
            account_id: CLOCK_01_PROGRAM_ACCOUNT_ID,
        }
    }

    fn ok_params() -> InitializeProgramParams<'static> {
        InitializeProgramParams {
            freeze_authority_account_id: freeze_id(),
            initial_stability_fee_per_millisecond: FIXED_POINT_ONE + 1_500_000_000_000_000,
            initial_controller_proportional_gain: 0,
            initial_controller_integral_gain: 0,
            initial_minimum_collateralization_ratio: FIXED_POINT_ONE * 3 / 2,
            minimum_milliseconds_between_rate_updates: 300_000,
            maximum_oracle_price_age_milliseconds: 900_000,
            initial_redemption_price: FIXED_POINT_ONE / 2,
            stablecoin_name: "test-stable",
        }
    }

    fn invoke(params: InitializeProgramParams<'_>) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
        initialize_program(
            admin_account(),
            uninit(protocol_parameters_id()),
            uninit(accumulator_id()),
            uninit(redemption_id()),
            uninit(stablecoin_def_id()),
            uninit(master_holding_id()),
            collateral_definition_account(),
            oracle_account(),
            clock_account(),
            STABLECOIN_PROGRAM_ID,
            params,
        )
    }

    #[test]
    fn happy_path_produces_nine_post_states_and_one_chained_call() {
        let (post_states, chained_calls) = invoke(ok_params());
        assert_eq!(post_states.len(), 9);
        assert_eq!(chained_calls.len(), 1);
    }

    #[test]
    fn happy_path_protocol_parameters_post_state_has_expected_fields() {
        let (post_states, _) = invoke(ok_params());
        let pp_post = &post_states[1];
        assert_eq!(
            pp_post.required_claim(),
            Some(Claim::Pda(compute_protocol_parameters_pda_seed()))
        );
        let decoded = ProtocolParameters::try_from(&pp_post.account().data).expect("decode");
        assert_eq!(decoded.admin_account_id, admin_id());
        assert_eq!(decoded.freeze_authority_account_id, freeze_id());
        assert_eq!(decoded.stablecoin_definition_id, stablecoin_def_id());
        assert_eq!(decoded.collateral_definition_id, collateral_definition_id());
        assert_eq!(decoded.market_price_oracle_id, oracle_id());
        assert!(!decoded.is_frozen);
    }

    #[test]
    fn happy_path_accumulator_starts_at_fixed_point_one_with_now() {
        let (post_states, _) = invoke(ok_params());
        let acc_post = &post_states[2];
        let decoded = StabilityFeeAccumulator::try_from(&acc_post.account().data).expect("decode");
        assert_eq!(decoded.accumulated_rate_at_last_accrual, FIXED_POINT_ONE);
        assert_eq!(decoded.last_accrued_at, NOW);
    }

    #[test]
    fn happy_path_redemption_state_starts_at_params_initial_redemption_price() {
        let (post_states, _) = invoke(ok_params());
        let rp_post = &post_states[3];
        let decoded = RedemptionPriceState::try_from(&rp_post.account().data).expect("decode");
        assert_eq!(decoded.redemption_price_at_last_update, FIXED_POINT_ONE / 2);
        assert_eq!(decoded.redemption_rate_per_millisecond, FIXED_POINT_ONE);
        assert_eq!(decoded.controller_integral_term, 0);
        assert_eq!(decoded.last_updated_at, NOW);
    }

    #[test]
    fn happy_path_clock_post_state_is_unchanged_and_unclaimed() {
        let (post_states, _) = invoke(ok_params());
        let clock_post = &post_states[8];
        assert_eq!(clock_post.required_claim(), None);
        assert_eq!(clock_post.account(), &clock_account().account);
    }

    #[test]
    fn happy_path_chained_call_is_new_fungible_definition_with_total_supply_zero_and_two_seeds() {
        let (_, chained_calls) = invoke(ok_params());
        let mut def_authorized = uninit(stablecoin_def_id());
        def_authorized.is_authorized = true;
        let mut master_authorized = uninit(master_holding_id());
        master_authorized.is_authorized = true;
        let expected = ChainedCall::new(
            TOKEN_PROGRAM_ID,
            vec![def_authorized, master_authorized],
            &token_core::Instruction::NewFungibleDefinition {
                name: "test-stable".to_owned(),
                total_supply: 0,
                mint_authority: Some(stablecoin_def_id()),
            },
        )
        .with_pda_seeds(vec![
            compute_stablecoin_definition_pda_seed(),
            compute_stablecoin_master_holding_pda_seed(),
        ]);
        assert_eq!(chained_calls[0], expected);
        assert_eq!(chained_calls[0].pda_seeds.len(), 2);
    }

    #[test]
    #[should_panic(expected = "Admin authorization is missing")]
    fn requires_admin_authorization() {
        let mut admin = admin_account();
        admin.is_authorized = false;
        let _ = initialize_program(
            admin,
            uninit(protocol_parameters_id()),
            uninit(accumulator_id()),
            uninit(redemption_id()),
            uninit(stablecoin_def_id()),
            uninit(master_holding_id()),
            collateral_definition_account(),
            oracle_account(),
            clock_account(),
            STABLECOIN_PROGRAM_ID,
            ok_params(),
        );
    }

    #[test]
    #[should_panic(expected = "ProtocolParameters account must be uninitialized")]
    fn rejects_initialized_protocol_parameters() {
        let mut pp = uninit(protocol_parameters_id());
        pp.account.program_owner = STABLECOIN_PROGRAM_ID;
        let _ = initialize_program(
            admin_account(),
            pp,
            uninit(accumulator_id()),
            uninit(redemption_id()),
            uninit(stablecoin_def_id()),
            uninit(master_holding_id()),
            collateral_definition_account(),
            oracle_account(),
            clock_account(),
            STABLECOIN_PROGRAM_ID,
            ok_params(),
        );
    }

    #[test]
    #[should_panic(expected = "StabilityFeeAccumulator account must be uninitialized")]
    fn rejects_initialized_accumulator() {
        let mut acc = uninit(accumulator_id());
        acc.account.program_owner = STABLECOIN_PROGRAM_ID;
        let _ = initialize_program(
            admin_account(),
            uninit(protocol_parameters_id()),
            acc,
            uninit(redemption_id()),
            uninit(stablecoin_def_id()),
            uninit(master_holding_id()),
            collateral_definition_account(),
            oracle_account(),
            clock_account(),
            STABLECOIN_PROGRAM_ID,
            ok_params(),
        );
    }

    #[test]
    #[should_panic(expected = "RedemptionPriceState account must be uninitialized")]
    fn rejects_initialized_redemption_state() {
        let mut rp = uninit(redemption_id());
        rp.account.program_owner = STABLECOIN_PROGRAM_ID;
        let _ = initialize_program(
            admin_account(),
            uninit(protocol_parameters_id()),
            uninit(accumulator_id()),
            rp,
            uninit(stablecoin_def_id()),
            uninit(master_holding_id()),
            collateral_definition_account(),
            oracle_account(),
            clock_account(),
            STABLECOIN_PROGRAM_ID,
            ok_params(),
        );
    }

    #[test]
    #[should_panic(expected = "StablecoinDefinition account must be uninitialized")]
    fn rejects_initialized_stablecoin_definition() {
        let mut sd = uninit(stablecoin_def_id());
        sd.account.program_owner = TOKEN_PROGRAM_ID;
        let _ = initialize_program(
            admin_account(),
            uninit(protocol_parameters_id()),
            uninit(accumulator_id()),
            uninit(redemption_id()),
            sd,
            uninit(master_holding_id()),
            collateral_definition_account(),
            oracle_account(),
            clock_account(),
            STABLECOIN_PROGRAM_ID,
            ok_params(),
        );
    }

    #[test]
    #[should_panic(expected = "StablecoinMasterHolding account must be uninitialized")]
    fn rejects_initialized_master_holding() {
        let mut mh = uninit(master_holding_id());
        mh.account.program_owner = TOKEN_PROGRAM_ID;
        let _ = initialize_program(
            admin_account(),
            uninit(protocol_parameters_id()),
            uninit(accumulator_id()),
            uninit(redemption_id()),
            uninit(stablecoin_def_id()),
            mh,
            collateral_definition_account(),
            oracle_account(),
            clock_account(),
            STABLECOIN_PROGRAM_ID,
            ok_params(),
        );
    }

    #[test]
    #[should_panic(
        expected = "ProtocolParameters account ID does not match expected PDA derivation"
    )]
    fn rejects_wrong_protocol_parameters_pda() {
        let pp = uninit(AccountId::new([0xDE; 32]));
        let _ = initialize_program(
            admin_account(),
            pp,
            uninit(accumulator_id()),
            uninit(redemption_id()),
            uninit(stablecoin_def_id()),
            uninit(master_holding_id()),
            collateral_definition_account(),
            oracle_account(),
            clock_account(),
            STABLECOIN_PROGRAM_ID,
            ok_params(),
        );
    }

    #[test]
    #[should_panic(
        expected = "StabilityFeeAccumulator account ID does not match expected PDA derivation"
    )]
    fn rejects_wrong_accumulator_pda() {
        let _ = initialize_program(
            admin_account(),
            uninit(protocol_parameters_id()),
            uninit(AccountId::new([0xDE; 32])),
            uninit(redemption_id()),
            uninit(stablecoin_def_id()),
            uninit(master_holding_id()),
            collateral_definition_account(),
            oracle_account(),
            clock_account(),
            STABLECOIN_PROGRAM_ID,
            ok_params(),
        );
    }

    #[test]
    #[should_panic(expected = "Clock account must be the system CLOCK_01 account")]
    fn rejects_wrong_clock_account() {
        let mut clock = clock_account();
        clock.account_id = AccountId::new([0xC1; 32]);
        let _ = initialize_program(
            admin_account(),
            uninit(protocol_parameters_id()),
            uninit(accumulator_id()),
            uninit(redemption_id()),
            uninit(stablecoin_def_id()),
            uninit(master_holding_id()),
            collateral_definition_account(),
            oracle_account(),
            clock,
            STABLECOIN_PROGRAM_ID,
            ok_params(),
        );
    }

    #[test]
    #[should_panic(expected = "Collateral definition account must be initialized")]
    fn rejects_uninitialized_collateral_definition() {
        let _ = initialize_program(
            admin_account(),
            uninit(protocol_parameters_id()),
            uninit(accumulator_id()),
            uninit(redemption_id()),
            uninit(stablecoin_def_id()),
            uninit(master_holding_id()),
            uninit(collateral_definition_id()),
            oracle_account(),
            clock_account(),
            STABLECOIN_PROGRAM_ID,
            ok_params(),
        );
    }

    #[test]
    #[should_panic(expected = "Market price oracle account must be initialized")]
    fn rejects_uninitialized_oracle() {
        let _ = initialize_program(
            admin_account(),
            uninit(protocol_parameters_id()),
            uninit(accumulator_id()),
            uninit(redemption_id()),
            uninit(stablecoin_def_id()),
            uninit(master_holding_id()),
            collateral_definition_account(),
            uninit(oracle_id()),
            clock_account(),
            STABLECOIN_PROGRAM_ID,
            ok_params(),
        );
    }

    #[test]
    #[should_panic(
        expected = "Oracle base_asset must equal the stablecoin definition's account_id"
    )]
    fn rejects_oracle_with_wrong_base_asset() {
        let mut oracle = oracle_account();
        let mut decoded = OraclePriceAccount::try_from(&oracle.account.data).unwrap();
        decoded.base_asset = AccountId::new([0xBA; 32]);
        oracle.account.data = Data::from(&decoded);
        let _ = initialize_program(
            admin_account(),
            uninit(protocol_parameters_id()),
            uninit(accumulator_id()),
            uninit(redemption_id()),
            uninit(stablecoin_def_id()),
            uninit(master_holding_id()),
            collateral_definition_account(),
            oracle,
            clock_account(),
            STABLECOIN_PROGRAM_ID,
            ok_params(),
        );
    }

    #[test]
    #[should_panic(
        expected = "Oracle quote_asset must equal the collateral definition's account_id"
    )]
    fn rejects_oracle_with_wrong_quote_asset() {
        let mut oracle = oracle_account();
        let mut decoded = OraclePriceAccount::try_from(&oracle.account.data).unwrap();
        decoded.quote_asset = AccountId::new([0xCC; 32]);
        oracle.account.data = Data::from(&decoded);
        let _ = initialize_program(
            admin_account(),
            uninit(protocol_parameters_id()),
            uninit(accumulator_id()),
            uninit(redemption_id()),
            uninit(stablecoin_def_id()),
            uninit(master_holding_id()),
            collateral_definition_account(),
            oracle,
            clock_account(),
            STABLECOIN_PROGRAM_ID,
            ok_params(),
        );
    }

    #[test]
    #[should_panic(expected = "initial_stability_fee_per_millisecond below FIXED_POINT_ONE")]
    fn rejects_stability_fee_below_fixed_point_one() {
        let mut p = ok_params();
        p.initial_stability_fee_per_millisecond = FIXED_POINT_ONE - 1;
        let _ = invoke(p);
    }

    #[test]
    #[should_panic(expected = "initial_stability_fee_per_millisecond above sane upper bound")]
    fn rejects_stability_fee_above_2x() {
        let mut p = ok_params();
        p.initial_stability_fee_per_millisecond = FIXED_POINT_ONE * 2 + 1;
        let _ = invoke(p);
    }

    #[test]
    #[should_panic(expected = "initial_minimum_collateralization_ratio below 1.1x")]
    fn rejects_collateralization_ratio_too_low() {
        let mut p = ok_params();
        p.initial_minimum_collateralization_ratio = FIXED_POINT_ONE;
        let _ = invoke(p);
    }

    #[test]
    #[should_panic(expected = "initial_minimum_collateralization_ratio above 10x")]
    fn rejects_collateralization_ratio_too_high() {
        let mut p = ok_params();
        p.initial_minimum_collateralization_ratio = FIXED_POINT_ONE * 11;
        let _ = invoke(p);
    }

    #[test]
    #[should_panic(expected = "minimum_milliseconds_between_rate_updates below minimum 1ms")]
    fn rejects_zero_rate_update_interval() {
        let mut p = ok_params();
        p.minimum_milliseconds_between_rate_updates = 0;
        let _ = invoke(p);
    }

    #[test]
    #[should_panic(expected = "maximum_oracle_price_age_milliseconds above maximum 86_400_000ms")]
    fn rejects_oracle_max_age_above_day() {
        let mut p = ok_params();
        p.maximum_oracle_price_age_milliseconds = 86_400_001;
        let _ = invoke(p);
    }

    #[test]
    #[should_panic(expected = "initial_redemption_price must be positive")]
    fn rejects_zero_initial_redemption_price() {
        let mut p = ok_params();
        p.initial_redemption_price = 0;
        let _ = invoke(p);
    }

    #[test]
    #[should_panic(expected = "stablecoin_name must be non-empty")]
    fn rejects_empty_stablecoin_name() {
        let mut p = ok_params();
        p.stablecoin_name = "";
        let _ = invoke(p);
    }
}
