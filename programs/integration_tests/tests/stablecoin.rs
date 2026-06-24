#![allow(
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration fixtures use fixed balances and transaction assertions"
)]

use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use nssa::{
    error::LeeError,
    program_deployment_transaction::{self, ProgramDeploymentTransaction},
    public_transaction, PrivateKey, PublicKey, PublicTransaction, V03State,
};
use nssa_core::account::{Account, AccountId, Data, Nonce};
use stablecoin_core::{
    compound_rate, compute_position_pda, compute_position_vault_pda,
    compute_protocol_parameters_pda, compute_redemption_price_state_pda,
    compute_stability_fee_accumulator_pda, compute_stablecoin_definition_pda,
    compute_stablecoin_master_holding_pda, Position, ProtocolParameters, RedemptionPriceState,
    StabilityFeeAccumulator, FIXED_POINT_ONE,
};
use token_core::{TokenDefinition, TokenHolding};
use twap_oracle_core::OraclePriceAccount;

const POSITION_NONCE: u64 = 7;
const CLOCK_START: u64 = 1_000;
const Q64_ONE: u128 = 18_446_744_073_709_551_616;

struct Keys;
struct Ids;
struct Balances;
struct Accounts;

impl Keys {
    fn admin() -> PrivateKey {
        PrivateKey::try_new([40; 32]).expect("valid private key")
    }

    fn owner() -> PrivateKey {
        PrivateKey::try_new([41; 32]).expect("valid private key")
    }

    fn user_collateral_holding() -> PrivateKey {
        PrivateKey::try_new([42; 32]).expect("valid private key")
    }

    fn user_stablecoin_holding() -> PrivateKey {
        PrivateKey::try_new([43; 32]).expect("valid private key")
    }
}

impl Ids {
    fn token_program() -> nssa_core::program::ProgramId {
        token_methods::TOKEN_ID
    }

    fn stablecoin_program() -> nssa_core::program::ProgramId {
        stablecoin_methods::STABLECOIN_ID
    }

    fn twap_oracle_program() -> nssa_core::program::ProgramId {
        twap_oracle_methods::TWAP_ORACLE_ID
    }

    fn admin() -> AccountId {
        AccountId::from(&PublicKey::new_from_private_key(&Keys::admin()))
    }

    fn freeze_authority() -> AccountId {
        AccountId::new([44; 32])
    }

    fn owner() -> AccountId {
        AccountId::from(&PublicKey::new_from_private_key(&Keys::owner()))
    }

    fn collateral_definition() -> AccountId {
        AccountId::new([5; 32])
    }

    fn user_collateral_holding() -> AccountId {
        AccountId::from(&PublicKey::new_from_private_key(
            &Keys::user_collateral_holding(),
        ))
    }

    fn oracle() -> AccountId {
        AccountId::new([7; 32])
    }

    fn protocol_parameters() -> AccountId {
        compute_protocol_parameters_pda(Self::stablecoin_program())
    }

    fn stability_fee_accumulator() -> AccountId {
        compute_stability_fee_accumulator_pda(Self::stablecoin_program())
    }

    fn redemption_price_state() -> AccountId {
        compute_redemption_price_state_pda(Self::stablecoin_program())
    }

    fn stablecoin_definition() -> AccountId {
        compute_stablecoin_definition_pda(Self::stablecoin_program())
    }

    fn stablecoin_master_holding() -> AccountId {
        compute_stablecoin_master_holding_pda(Self::stablecoin_program())
    }

    fn user_stablecoin_holding() -> AccountId {
        AccountId::from(&PublicKey::new_from_private_key(
            &Keys::user_stablecoin_holding(),
        ))
    }

    fn position() -> AccountId {
        compute_position_pda(Self::stablecoin_program(), Self::owner(), POSITION_NONCE)
    }

    fn vault() -> AccountId {
        compute_position_vault_pda(Self::stablecoin_program(), Self::position())
    }
}

impl Balances {
    fn user_collateral_init() -> u128 {
        1_000_000
    }

    fn collateral_deposit() -> u128 {
        500
    }

    fn collateral_withdraw() -> u128 {
        400
    }

    fn generated_debt() -> u128 {
        100
    }
}

impl Accounts {
    fn collateral_definition() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0,
            data: Data::from(&TokenDefinition::Fungible {
                name: String::from("Gold"),
                total_supply: Balances::user_collateral_init(),
                metadata_id: None,
                authority: None,
            }),
            nonce: Nonce(0),
        }
    }

    fn user_collateral_holding() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::collateral_definition(),
                balance: Balances::user_collateral_init(),
            }),
            nonce: Nonce(0),
        }
    }

    fn user_stablecoin_holding() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::stablecoin_definition(),
                balance: 0,
            }),
            nonce: Nonce(0),
        }
    }

    fn oracle(timestamp: u64) -> Account {
        Account {
            program_owner: Ids::twap_oracle_program(),
            balance: 0,
            data: Data::from(&OraclePriceAccount {
                base_asset: Ids::stablecoin_definition(),
                quote_asset: Ids::collateral_definition(),
                price: Q64_ONE,
                timestamp,
                source_id: AccountId::new([8; 32]),
                confidence_interval: 0,
            }),
            nonce: Nonce(0),
        }
    }
}

fn deploy_programs(state: &mut V03State) {
    let token_message =
        program_deployment_transaction::Message::new(token_methods::TOKEN_ELF.to_vec());
    state
        .transition_from_program_deployment_transaction(&ProgramDeploymentTransaction::new(
            token_message,
        ))
        .expect("token program deployment must succeed");

    let stablecoin_message =
        program_deployment_transaction::Message::new(stablecoin_methods::STABLECOIN_ELF.to_vec());
    state
        .transition_from_program_deployment_transaction(&ProgramDeploymentTransaction::new(
            stablecoin_message,
        ))
        .expect("stablecoin program deployment must succeed");
}

fn state_for_stablecoin_tests() -> V03State {
    let mut state = V03State::new();
    deploy_programs(&mut state);
    state.force_insert_account(
        Ids::collateral_definition(),
        Accounts::collateral_definition(),
    );
    state.force_insert_account(
        Ids::user_collateral_holding(),
        Accounts::user_collateral_holding(),
    );
    state.force_insert_account(
        Ids::user_stablecoin_holding(),
        Accounts::user_stablecoin_holding(),
    );
    state.force_insert_account(Ids::oracle(), Accounts::oracle(CLOCK_START));
    advance_clock(&mut state, CLOCK_START);
    state
}

fn current_nonce(state: &V03State, account_id: AccountId) -> Nonce {
    state.get_account_by_id(account_id).nonce
}

fn advance_clock(state: &mut V03State, timestamp: u64) {
    let data = ClockAccountData {
        block_id: 0,
        timestamp,
    }
    .to_bytes();
    let clock_account = Account {
        data: Data::try_from(data).expect("clock account data fits"),
        ..Account::default()
    };
    state.force_insert_account(CLOCK_01_PROGRAM_ACCOUNT_ID, clock_account);
}

fn initialize_protocol(state: &mut V03State, rate: u128) {
    let instruction = stablecoin_core::Instruction::InitializeProgram {
        freeze_authority_account_id: Ids::freeze_authority(),
        initial_stability_fee_per_millisecond: rate,
        initial_controller_proportional_gain: 0,
        initial_controller_integral_gain: 0,
        initial_minimum_collateralization_ratio: FIXED_POINT_ONE / 10 * 11,
        minimum_milliseconds_between_rate_updates: 1,
        maximum_oracle_price_age_milliseconds: 86_400_000,
        initial_redemption_price: FIXED_POINT_ONE,
        stablecoin_name: String::from("DAI"),
    };
    let message = public_transaction::Message::try_new(
        Ids::stablecoin_program(),
        vec![
            Ids::admin(),
            Ids::protocol_parameters(),
            Ids::stability_fee_accumulator(),
            Ids::redemption_price_state(),
            Ids::stablecoin_definition(),
            Ids::stablecoin_master_holding(),
            Ids::collateral_definition(),
            Ids::oracle(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        vec![current_nonce(state, Ids::admin())],
        instruction,
    )
    .unwrap();
    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::admin()]);
    let tx = PublicTransaction::new(message, witness_set);
    state
        .transition_from_public_transaction(&tx, 0, 0)
        .expect("initialize_program must succeed");
}

fn open_position(state: &mut V03State) {
    let instruction = stablecoin_core::Instruction::OpenPosition {
        position_nonce: POSITION_NONCE,
        initial_collateral_amount: Balances::collateral_deposit(),
    };
    let message = public_transaction::Message::try_new(
        Ids::stablecoin_program(),
        vec![
            Ids::owner(),
            Ids::position(),
            Ids::vault(),
            Ids::user_collateral_holding(),
            Ids::collateral_definition(),
            Ids::protocol_parameters(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        vec![
            current_nonce(state, Ids::owner()),
            current_nonce(state, Ids::user_collateral_holding()),
        ],
        instruction,
    )
    .unwrap();
    let witness_set = public_transaction::WitnessSet::for_message(
        &message,
        &[&Keys::owner(), &Keys::user_collateral_holding()],
    );
    let tx = PublicTransaction::new(message, witness_set);
    state
        .transition_from_public_transaction(&tx, 0, 0)
        .expect("open_position must succeed");
}

fn generate_debt(state: &mut V03State) {
    let instruction = stablecoin_core::Instruction::GenerateDebt {
        amount: Balances::generated_debt(),
    };
    let message = public_transaction::Message::try_new(
        Ids::stablecoin_program(),
        vec![
            Ids::owner(),
            Ids::position(),
            Ids::stablecoin_definition(),
            Ids::user_stablecoin_holding(),
            Ids::stability_fee_accumulator(),
            Ids::redemption_price_state(),
            Ids::oracle(),
            Ids::protocol_parameters(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        vec![current_nonce(state, Ids::owner())],
        instruction,
    )
    .unwrap();
    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::owner()]);
    let tx = PublicTransaction::new(message, witness_set);
    state
        .transition_from_public_transaction(&tx, 0, 0)
        .expect("generate_debt must succeed");
}

fn accrue_stability_fee(state: &mut V03State) {
    execute_accrue_stability_fee(state).expect("accrue_stability_fee must succeed");
}

fn execute_accrue_stability_fee(state: &mut V03State) -> Result<(), LeeError> {
    let instruction = stablecoin_core::Instruction::AccrueStabilityFee;
    let message = public_transaction::Message::try_new(
        Ids::stablecoin_program(),
        vec![
            Ids::owner(),
            Ids::protocol_parameters(),
            Ids::stability_fee_accumulator(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        vec![current_nonce(state, Ids::owner())],
        instruction,
    )
    .unwrap();
    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::owner()]);
    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0, 0)
}

fn set_stability_fee_per_millisecond(state: &mut V03State, new_rate: u128) {
    let instruction = stablecoin_core::Instruction::SetStabilityFeePerMillisecond { new_rate };
    let message = public_transaction::Message::try_new(
        Ids::stablecoin_program(),
        vec![
            Ids::admin(),
            Ids::protocol_parameters(),
            Ids::stability_fee_accumulator(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        vec![current_nonce(state, Ids::admin())],
        instruction,
    )
    .unwrap();
    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::admin()]);
    let tx = PublicTransaction::new(message, witness_set);
    state
        .transition_from_public_transaction(&tx, 0, 0)
        .expect("set_stability_fee_per_millisecond must succeed");
}

fn repay_debt(state: &mut V03State) {
    let instruction = stablecoin_core::Instruction::RepayDebt {
        amount: Balances::generated_debt(),
    };
    let message = public_transaction::Message::try_new(
        Ids::stablecoin_program(),
        vec![
            Ids::owner(),
            Ids::position(),
            Ids::stablecoin_definition(),
            Ids::user_stablecoin_holding(),
            Ids::stability_fee_accumulator(),
            Ids::protocol_parameters(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        vec![
            current_nonce(state, Ids::owner()),
            current_nonce(state, Ids::user_stablecoin_holding()),
        ],
        instruction,
    )
    .unwrap();
    let witness_set = public_transaction::WitnessSet::for_message(
        &message,
        &[&Keys::owner(), &Keys::user_stablecoin_holding()],
    );
    let tx = PublicTransaction::new(message, witness_set);
    state
        .transition_from_public_transaction(&tx, 0, 0)
        .expect("repay_debt must succeed");
}

fn withdraw_collateral(state: &mut V03State) {
    execute_withdraw_collateral(state, Balances::collateral_withdraw())
        .expect("withdraw_collateral must succeed");
}

fn execute_withdraw_collateral(state: &mut V03State, amount: u128) -> Result<(), LeeError> {
    let instruction = stablecoin_core::Instruction::WithdrawCollateral { amount };
    let message = public_transaction::Message::try_new(
        Ids::stablecoin_program(),
        vec![
            Ids::owner(),
            Ids::position(),
            Ids::vault(),
            Ids::user_collateral_holding(),
            Ids::stability_fee_accumulator(),
            Ids::redemption_price_state(),
            Ids::protocol_parameters(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        vec![current_nonce(state, Ids::owner())],
        instruction,
    )
    .unwrap();
    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::owner()]);
    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0, 0)
}

fn position(state: &V03State) -> Position {
    Position::try_from(&state.get_account_by_id(Ids::position()).data).expect("valid Position")
}

fn stability_fee_accumulator(state: &V03State) -> StabilityFeeAccumulator {
    StabilityFeeAccumulator::try_from(
        &state
            .get_account_by_id(Ids::stability_fee_accumulator())
            .data,
    )
    .expect("valid StabilityFeeAccumulator")
}

fn protocol_parameters(state: &V03State) -> ProtocolParameters {
    ProtocolParameters::try_from(&state.get_account_by_id(Ids::protocol_parameters()).data)
        .expect("valid ProtocolParameters")
}

fn redemption_price_state(state: &V03State) -> RedemptionPriceState {
    RedemptionPriceState::try_from(&state.get_account_by_id(Ids::redemption_price_state()).data)
        .expect("valid RedemptionPriceState")
}

fn fungible_balance(state: &V03State, account_id: AccountId) -> u128 {
    match TokenHolding::try_from(&state.get_account_by_id(account_id).data)
        .expect("valid TokenHolding")
    {
        TokenHolding::Fungible { balance, .. } => balance,
        TokenHolding::NftMaster { .. } | TokenHolding::NftPrintedCopy { .. } => {
            panic!("expected Fungible holding")
        }
    }
}

fn fungible_supply(state: &V03State, account_id: AccountId) -> u128 {
    match TokenDefinition::try_from(&state.get_account_by_id(account_id).data)
        .expect("valid TokenDefinition")
    {
        TokenDefinition::Fungible { total_supply, .. } => total_supply,
        TokenDefinition::NonFungible { .. } => panic!("expected Fungible definition"),
    }
}

#[test]
fn stablecoin_initialize_then_accrue_fee() {
    let mut state = state_for_stablecoin_tests();
    let rate = FIXED_POINT_ONE + FIXED_POINT_ONE / 10;
    initialize_protocol(&mut state, rate);

    let params = protocol_parameters(&state);
    assert_eq!(params.stability_fee_per_millisecond, rate);
    assert_eq!(
        params.stablecoin_definition_id,
        Ids::stablecoin_definition()
    );
    assert_eq!(
        params.collateral_definition_id,
        Ids::collateral_definition()
    );
    assert_eq!(params.market_price_oracle_id, Ids::oracle());
    assert_eq!(fungible_supply(&state, Ids::stablecoin_definition()), 0);

    let accumulator = stability_fee_accumulator(&state);
    assert_eq!(
        accumulator.accumulated_rate_at_last_accrual,
        FIXED_POINT_ONE
    );
    assert_eq!(accumulator.last_accrued_at, CLOCK_START);
    let redemption = redemption_price_state(&state);
    assert_eq!(redemption.redemption_price_at_last_update, FIXED_POINT_ONE);
    assert_eq!(redemption.last_updated_at, CLOCK_START);

    advance_clock(&mut state, CLOCK_START + 2);
    accrue_stability_fee(&mut state);

    let updated = stability_fee_accumulator(&state);
    assert_eq!(
        updated.accumulated_rate_at_last_accrual,
        compound_rate(rate, 2)
    );
    assert_eq!(updated.last_accrued_at, CLOCK_START + 2);
}

#[test]
fn stablecoin_set_fee_rate_anchors_old_rate_before_switching() {
    let mut state = state_for_stablecoin_tests();
    let old_rate = FIXED_POINT_ONE + FIXED_POINT_ONE / 10;
    initialize_protocol(&mut state, old_rate);

    advance_clock(&mut state, CLOCK_START + 2);
    set_stability_fee_per_millisecond(&mut state, FIXED_POINT_ONE);

    let updated = stability_fee_accumulator(&state);
    let old_window_anchor = compound_rate(old_rate, 2);
    assert_eq!(updated.accumulated_rate_at_last_accrual, old_window_anchor);
    assert_eq!(updated.last_accrued_at, CLOCK_START + 2);
    assert_eq!(
        protocol_parameters(&state).stability_fee_per_millisecond,
        FIXED_POINT_ONE
    );

    advance_clock(&mut state, CLOCK_START + 4);
    accrue_stability_fee(&mut state);
    let reanchored = stability_fee_accumulator(&state);
    assert_eq!(
        reanchored.accumulated_rate_at_last_accrual,
        old_window_anchor
    );
    assert_eq!(reanchored.last_accrued_at, CLOCK_START + 4);
}

#[test]
fn stablecoin_open_generate_repay_and_withdraw_flow() {
    let mut state = state_for_stablecoin_tests();
    initialize_protocol(&mut state, FIXED_POINT_ONE);

    open_position(&mut state);
    let opened = position(&state);
    assert_eq!(opened.owner_account_id, Ids::owner());
    assert_eq!(opened.position_nonce, POSITION_NONCE);
    assert_eq!(opened.vault_account_id, Ids::vault());
    assert_eq!(opened.collateral_amount, Balances::collateral_deposit());
    assert_eq!(opened.normalized_debt_amount, 0);
    assert_eq!(opened.opened_at, CLOCK_START);
    assert_eq!(
        fungible_balance(&state, Ids::user_collateral_holding()),
        Balances::user_collateral_init() - Balances::collateral_deposit()
    );
    assert_eq!(
        fungible_balance(&state, Ids::vault()),
        Balances::collateral_deposit()
    );

    generate_debt(&mut state);
    advance_clock(&mut state, CLOCK_START + 1);
    accrue_stability_fee(&mut state);
    let indebted = position(&state);
    assert_eq!(indebted.normalized_debt_amount, Balances::generated_debt());
    assert_eq!(
        fungible_supply(&state, Ids::stablecoin_definition()),
        Balances::generated_debt()
    );
    assert_eq!(
        fungible_balance(&state, Ids::user_stablecoin_holding()),
        Balances::generated_debt()
    );

    repay_debt(&mut state);
    let repaid = position(&state);
    assert_eq!(repaid.normalized_debt_amount, 0);
    assert_eq!(fungible_supply(&state, Ids::stablecoin_definition()), 0);
    assert_eq!(fungible_balance(&state, Ids::user_stablecoin_holding()), 0);

    withdraw_collateral(&mut state);
    let withdrawn = position(&state);
    assert_eq!(
        withdrawn.collateral_amount,
        Balances::collateral_deposit() - Balances::collateral_withdraw()
    );
    assert_eq!(
        fungible_balance(&state, Ids::vault()),
        Balances::collateral_deposit() - Balances::collateral_withdraw()
    );
    assert_eq!(
        fungible_balance(&state, Ids::user_collateral_holding()),
        Balances::user_collateral_init() - Balances::collateral_deposit()
            + Balances::collateral_withdraw()
    );
}

#[test]
fn stablecoin_debt_bearing_withdraw_checks_collateralization() {
    let mut state = state_for_stablecoin_tests();
    initialize_protocol(&mut state, FIXED_POINT_ONE);
    open_position(&mut state);
    generate_debt(&mut state);

    assert!(matches!(
        execute_withdraw_collateral(&mut state, 400),
        Err(LeeError::ProgramExecutionFailed(_))
    ));
    assert_eq!(
        position(&state).collateral_amount,
        Balances::collateral_deposit()
    );

    execute_withdraw_collateral(&mut state, 300)
        .expect("safe debt-bearing withdrawal must succeed");
    assert_eq!(position(&state).collateral_amount, 200);
    assert_eq!(fungible_balance(&state, Ids::vault()), 200);
}
