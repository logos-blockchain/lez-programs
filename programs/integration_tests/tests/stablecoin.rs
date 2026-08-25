use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use lee::{
    error::LeeError,
    program_deployment_transaction::{self, ProgramDeploymentTransaction},
    public_transaction, PrivateKey, PublicKey, PublicTransaction, V03State,
};
use lee_core::account::{Account, AccountId, Data, Nonce};
use stablecoin_core::{
    compute_position_pda, compute_position_vault_pda, compute_protocol_parameters_pda,
    compute_redemption_price_state_pda, compute_stability_fee_accumulator_pda,
    compute_stablecoin_definition_pda, compute_stablecoin_master_holding_pda, Position,
};
use token_core::{TokenDefinition, TokenHolding};

struct Keys;
struct Ids;
struct Balances;
struct Accounts;

impl Keys {
    fn owner() -> PrivateKey {
        PrivateKey::try_new([41; 32]).expect("valid private key")
    }

    fn user_holding() -> PrivateKey {
        PrivateKey::try_new([42; 32]).expect("valid private key")
    }

    fn user_stablecoin_holding() -> PrivateKey {
        PrivateKey::try_new([43; 32]).expect("valid private key")
    }

    fn admin() -> PrivateKey {
        PrivateKey::try_new([44; 32]).expect("valid private key")
    }
}

impl Ids {
    fn token_program() -> lee_core::program::ProgramId {
        token_methods::TOKEN_ID
    }

    fn stablecoin_program() -> lee_core::program::ProgramId {
        stablecoin_methods::STABLECOIN_ID
    }

    fn collateral_definition() -> AccountId {
        AccountId::new([5; 32])
    }

    fn owner() -> AccountId {
        AccountId::from(&PublicKey::new_from_private_key(&Keys::owner()))
    }

    fn user_holding() -> AccountId {
        AccountId::from(&PublicKey::new_from_private_key(&Keys::user_holding()))
    }

    fn stablecoin_definition() -> AccountId {
        AccountId::new([6; 32])
    }

    fn user_stablecoin_holding() -> AccountId {
        AccountId::from(&PublicKey::new_from_private_key(
            &Keys::user_stablecoin_holding(),
        ))
    }

    fn position_nonce() -> u64 {
        0
    }

    fn position() -> AccountId {
        compute_position_pda(
            Self::stablecoin_program(),
            Self::owner(),
            Self::position_nonce(),
        )
    }

    fn vault() -> AccountId {
        compute_position_vault_pda(Self::stablecoin_program(), Self::position())
    }

    fn admin() -> AccountId {
        AccountId::from(&PublicKey::new_from_private_key(&Keys::admin()))
    }

    fn freeze_authority() -> AccountId {
        AccountId::new([0xFE; 32])
    }

    fn oracle() -> AccountId {
        AccountId::new([0x70; 32])
    }

    /// The stablecoin's `TokenDefinition` PDA created by `initialize_program`
    /// (distinct from `stablecoin_definition`, the externally-owned definition
    /// the repay test uses).
    fn stablecoin_definition_pda() -> AccountId {
        compute_stablecoin_definition_pda(Self::stablecoin_program())
    }
}

impl Balances {
    fn user_holding_init() -> u128 {
        1_000_000
    }

    fn collateral_deposit() -> u128 {
        500_000
    }

    fn collateral_withdraw() -> u128 {
        200_000
    }

    fn stablecoin_supply_init() -> u128 {
        1_000
    }

    fn user_stablecoin_holding_init() -> u128 {
        1_000
    }

    fn initial_debt() -> u128 {
        300
    }

    fn debt_repay_amount() -> u128 {
        100
    }
}

impl Accounts {
    fn collateral_definition_init() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenDefinition::Fungible {
                name: String::from("Gold"),
                total_supply: Balances::user_holding_init(),
                metadata_id: None,
                authority: None,
            }),
            nonce: Nonce(0),
        }
    }

    fn user_holding_init() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::collateral_definition(),
                balance: Balances::user_holding_init(),
            }),
            nonce: Nonce(0),
        }
    }

    fn stablecoin_definition_init() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenDefinition::Fungible {
                name: String::from("DAI"),
                total_supply: Balances::stablecoin_supply_init(),
                metadata_id: None,
                authority: None,
            }),
            nonce: Nonce(0),
        }
    }

    fn user_stablecoin_holding_init() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::stablecoin_definition(),
                balance: Balances::user_stablecoin_holding_init(),
            }),
            nonce: Nonce(0),
        }
    }

    fn position_with_debt_init() -> Account {
        Account {
            program_owner: stablecoin_methods::STABLECOIN_ID,
            balance: 0_u128,
            data: Data::from(&Position {
                owner_account_id: Ids::owner(),
                position_nonce: Ids::position_nonce(),
                vault_account_id: Ids::vault(),
                collateral_amount: Balances::collateral_deposit(),
                normalized_debt_amount: Balances::initial_debt(),
                opened_at: 0,
            }),
            nonce: Nonce(0),
        }
    }

    fn oracle_init(base_asset: AccountId, quote_asset: AccountId) -> Account {
        Self::oracle_with(
            base_asset,
            quote_asset,
            stablecoin_core::math::FIXED_POINT_ONE / 2,
            0,
        )
    }

    /// An oracle observation at an explicit price and timestamp — the poke tests
    /// need both to control the controller's error term and the freshness gate.
    fn oracle_with(
        base_asset: AccountId,
        quote_asset: AccountId,
        price: u128,
        timestamp: u64,
    ) -> Account {
        Account {
            program_owner: [9u32; 8],
            balance: 0_u128,
            data: Data::from(&twap_oracle_core::OraclePriceAccount {
                base_asset,
                quote_asset,
                price,
                timestamp,
                source_id: Ids::oracle(),
                confidence_interval: 0,
            }),
            nonce: Nonce(0),
        }
    }
}

/// Seeds the canonical `CLOCK_01` account at `timestamp`. `V03State::new()` no longer
/// auto-creates it, and `initialize_program` reads it for wall-clock time.
fn seed_clock(state: &mut V03State, timestamp: u64) {
    let data = ClockAccountData {
        block_id: 0,
        timestamp,
    }
    .to_bytes();
    let clock_account = Account {
        // The real CLOCK_01 system account is owned by the clock program, not the
        // default program (see lee `system_accounts::clock_account`). A default owner
        // makes the spel-framework output filter drop the (unchanged, unclaimed)
        // clock post-state, which v0.2.1's DeclaredAccountMissingFromOutput invariant
        // then rejects. Use a non-default placeholder owner, as the oracle fixture does.
        program_owner: [8u32; 8],
        data: Data::try_from(data).expect("clock account data fits"),
        ..Account::default()
    };
    state.force_insert_account(CLOCK_01_PROGRAM_ACCOUNT_ID, clock_account);
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
        Accounts::collateral_definition_init(),
    );
    state.force_insert_account(Ids::user_holding(), Accounts::user_holding_init());
    // Seed the owner as a non-default-owned account. In lee, balance-holding user
    // accounts are owned by a system program (see lee `system_accounts` / the
    // `time_locked_transfer` sender), not the default program. A default-owned owner
    // works the first time it signs (its pre-state is still `Account::default()`), but
    // once `open_position` bumps its nonce the spel-framework output filter drops the
    // (unchanged, unclaimed, default-owned) owner post-state from the second
    // transaction — which v0.2.1's DeclaredAccountMissingFromOutput invariant then
    // rejects. A non-default owner keeps it in the diff across both transactions.
    state.force_insert_account(
        Ids::owner(),
        Account {
            program_owner: [7u32; 8],
            ..Account::default()
        },
    );
    state
}

fn current_nonce(state: &V03State, account_id: AccountId) -> Nonce {
    state.get_account_by_id(account_id).nonce
}

fn state_for_stablecoin_repay_tests() -> V03State {
    let mut state = V03State::new();
    deploy_programs(&mut state);
    state.force_insert_account(
        Ids::collateral_definition(),
        Accounts::collateral_definition_init(),
    );
    state.force_insert_account(
        Ids::stablecoin_definition(),
        Accounts::stablecoin_definition_init(),
    );
    state.force_insert_account(Ids::position(), Accounts::position_with_debt_init());
    state.force_insert_account(
        Ids::user_stablecoin_holding(),
        Accounts::user_stablecoin_holding_init(),
    );
    state
}

fn assert_position(state: &V03State, expected_collateral: u128) {
    let position =
        Position::try_from(&state.get_account_by_id(Ids::position()).data).expect("valid Position");
    assert_eq!(position.collateral_amount, expected_collateral);
    assert_eq!(position.normalized_debt_amount, 0);
    assert_eq!(position.vault_account_id, Ids::vault());
    assert_eq!(position.owner_account_id, Ids::owner());
    assert_eq!(position.position_nonce, Ids::position_nonce());
}

fn assert_fungible_balance(state: &V03State, account_id: AccountId, expected_balance: u128) {
    let holding = TokenHolding::try_from(&state.get_account_by_id(account_id).data)
        .expect("valid TokenHolding");
    match holding {
        TokenHolding::Fungible {
            definition_id,
            balance,
        } => {
            assert_eq!(definition_id, Ids::collateral_definition());
            assert_eq!(balance, expected_balance);
        }
        TokenHolding::NftMaster { .. } | TokenHolding::NftPrintedCopy { .. } => {
            panic!("expected Fungible holding")
        }
    }
}

#[test]
fn stablecoin_open_position_then_withdraw_collateral() {
    let mut state = state_for_stablecoin_tests();

    // Open the position: deposit collateral from the user's holding into a fresh vault.
    let open = stablecoin_core::Instruction::OpenPosition {
        position_nonce: Ids::position_nonce(),
        collateral_amount: Balances::collateral_deposit(),
    };
    let message = public_transaction::Message::try_new(
        Ids::stablecoin_program(),
        vec![
            Ids::owner(),
            Ids::position(),
            Ids::vault(),
            Ids::user_holding(),
            Ids::collateral_definition(),
        ],
        vec![
            current_nonce(&state, Ids::owner()),
            current_nonce(&state, Ids::user_holding()),
        ],
        open,
    )
    .unwrap();
    let witness_set = public_transaction::WitnessSet::for_message(
        &message,
        &[&Keys::owner(), &Keys::user_holding()],
    );
    let tx = PublicTransaction::new(message, witness_set);
    state
        .transition_from_public_transaction(&tx, 0, 0)
        .expect("open_position must succeed");

    assert_position(&state, Balances::collateral_deposit());
    assert_fungible_balance(&state, Ids::vault(), Balances::collateral_deposit());
    assert_fungible_balance(
        &state,
        Ids::user_holding(),
        Balances::user_holding_init() - Balances::collateral_deposit(),
    );

    // Withdraw part of the collateral back to the same user holding.
    let withdraw = stablecoin_core::Instruction::WithdrawCollateral {
        amount: Balances::collateral_withdraw(),
    };
    let message = public_transaction::Message::try_new(
        Ids::stablecoin_program(),
        vec![
            Ids::owner(),
            Ids::position(),
            Ids::vault(),
            Ids::user_holding(),
        ],
        vec![current_nonce(&state, Ids::owner())],
        withdraw,
    )
    .unwrap();
    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::owner()]);
    let tx = PublicTransaction::new(message, witness_set);
    state
        .transition_from_public_transaction(&tx, 0, 0)
        .expect("withdraw_collateral must succeed");

    assert_position(
        &state,
        Balances::collateral_deposit() - Balances::collateral_withdraw(),
    );
    assert_fungible_balance(
        &state,
        Ids::vault(),
        Balances::collateral_deposit() - Balances::collateral_withdraw(),
    );
    assert_fungible_balance(
        &state,
        Ids::user_holding(),
        Balances::user_holding_init() - Balances::collateral_deposit()
            + Balances::collateral_withdraw(),
    );
}

#[test]
fn stablecoin_repay_debt_burns_stablecoins_and_decreases_debt() {
    let mut state = state_for_stablecoin_repay_tests();

    let repay = stablecoin_core::Instruction::RepayDebt {
        amount: Balances::debt_repay_amount(),
    };
    let message = public_transaction::Message::try_new(
        Ids::stablecoin_program(),
        vec![
            Ids::owner(),
            Ids::position(),
            Ids::stablecoin_definition(),
            Ids::user_stablecoin_holding(),
        ],
        vec![
            current_nonce(&state, Ids::owner()),
            current_nonce(&state, Ids::user_stablecoin_holding()),
        ],
        repay,
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

    // Position debt decreased; collateral untouched.
    let position =
        Position::try_from(&state.get_account_by_id(Ids::position()).data).expect("valid Position");
    assert_eq!(
        position.normalized_debt_amount,
        Balances::initial_debt() - Balances::debt_repay_amount()
    );
    assert_eq!(position.collateral_amount, Balances::collateral_deposit());

    // Stablecoin total supply decreased by the burn amount.
    let definition =
        TokenDefinition::try_from(&state.get_account_by_id(Ids::stablecoin_definition()).data)
            .expect("valid TokenDefinition");
    match definition {
        TokenDefinition::Fungible { total_supply, .. } => {
            assert_eq!(
                total_supply,
                Balances::stablecoin_supply_init() - Balances::debt_repay_amount()
            );
        }
        TokenDefinition::NonFungible { .. } => panic!("expected Fungible definition"),
    }

    // User stablecoin holding decreased by the burn amount.
    let holding =
        TokenHolding::try_from(&state.get_account_by_id(Ids::user_stablecoin_holding()).data)
            .expect("valid TokenHolding");
    match holding {
        TokenHolding::Fungible { balance, .. } => {
            assert_eq!(
                balance,
                Balances::user_stablecoin_holding_init() - Balances::debt_repay_amount()
            );
        }
        TokenHolding::NftMaster { .. } | TokenHolding::NftPrintedCopy { .. } => {
            panic!("expected Fungible holding")
        }
    }
}

/// Protocol parameters the initialized-protocol helper installs. Kept as
/// constants so the poke tests can reason about the interval / staleness gates.
mod protocol_config {
    use stablecoin_core::math::FIXED_POINT_ONE;

    /// ~5% annual, expressed per millisecond.
    pub(super) const STABILITY_FEE_PER_MILLISECOND: u128 = FIXED_POINT_ONE + 1_500_000_000_000_000;
    pub(super) const MINIMUM_MILLISECONDS_BETWEEN_RATE_UPDATES: u64 = 300_000;
    pub(super) const MAXIMUM_ORACLE_PRICE_AGE_MILLISECONDS: u64 = 900_000;
    pub(super) const INITIAL_REDEMPTION_PRICE: u128 = FIXED_POINT_ONE / 2;
}

/// Deploys both programs and runs `InitializeProgram` at `now`, leaving a fully
/// bootstrapped protocol. Shared by the init test and all three poke tests.
///
/// `controller_proportional_gain` is a parameter because the poke tests need a
/// live controller (a zero gain pins the redemption rate at `FIXED_POINT_ONE`,
/// which would make the update assertions vacuous).
fn initialize_protocol(now: u64, controller_proportional_gain: i128) -> V03State {
    use stablecoin_core::math::FIXED_POINT_ONE;

    // `V03State::new()` no longer auto-creates the clock account; seed CLOCK_01 with this
    // timestamp, which initialize_program reads as `now` to anchor the accumulator and
    // redemption-price state.
    let mut state = V03State::new();
    seed_clock(&mut state, now);
    deploy_programs(&mut state);

    // Externally-created collateral definition + market-price oracle.
    state.force_insert_account(
        Ids::collateral_definition(),
        Accounts::collateral_definition_init(),
    );
    state.force_insert_account(
        Ids::oracle(),
        Accounts::oracle_init(
            Ids::stablecoin_definition_pda(),
            Ids::collateral_definition(),
        ),
    );
    // Seed the admin (the initialize + poke signer) as a non-default-owned account. A
    // default-owned signer works on its first sign (pre-state is still Account::default()),
    // but once its nonce bumps the spel-framework output filter drops it (non-default state
    // + default owner + no claim), tripping DeclaredAccountMissingFromOutput on the next tx
    // (the pokes). A non-default owner keeps it in every diff. Same fix as Ids::owner().
    state.force_insert_account(
        Ids::admin(),
        Account {
            program_owner: [7u32; 8],
            ..Account::default()
        },
    );

    let instruction = stablecoin_core::Instruction::InitializeProgram {
        freeze_authority_account_id: Ids::freeze_authority(),
        initial_stability_fee_per_millisecond: protocol_config::STABILITY_FEE_PER_MILLISECOND,
        initial_controller_proportional_gain: controller_proportional_gain,
        initial_controller_integral_gain: 0,
        initial_minimum_collateralization_ratio: FIXED_POINT_ONE * 3 / 2,
        minimum_milliseconds_between_rate_updates:
            protocol_config::MINIMUM_MILLISECONDS_BETWEEN_RATE_UPDATES,
        maximum_oracle_price_age_milliseconds:
            protocol_config::MAXIMUM_ORACLE_PRICE_AGE_MILLISECONDS,
        initial_redemption_price: protocol_config::INITIAL_REDEMPTION_PRICE,
        stablecoin_name: String::from("test-stable"),
    };

    let message = public_transaction::Message::try_new(
        Ids::stablecoin_program(),
        vec![
            Ids::admin(),
            compute_protocol_parameters_pda(Ids::stablecoin_program()),
            compute_stability_fee_accumulator_pda(Ids::stablecoin_program()),
            compute_redemption_price_state_pda(Ids::stablecoin_program()),
            Ids::stablecoin_definition_pda(),
            compute_stablecoin_master_holding_pda(Ids::stablecoin_program()),
            Ids::collateral_definition(),
            Ids::oracle(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        vec![current_nonce(&state, Ids::admin())],
        instruction,
    )
    .expect("valid initialize_program message");
    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::admin()]);
    let tx = PublicTransaction::new(message, witness_set);
    state
        .transition_from_public_transaction(&tx, 1, now)
        .expect("initialize_program must succeed");

    state
}

/// Submits a no-parameter poke signed by the admin (pokes are permissionless —
/// the admin key is just a convenient signer) and advances the clock to `now`
/// first, so the guest reads the intended timestamp.
fn submit_poke(
    state: &mut V03State,
    now: u64,
    block_id: u64,
    instruction: stablecoin_core::Instruction,
    accounts: Vec<AccountId>,
) -> Result<(), LeeError> {
    seed_clock(state, now);
    let mut account_ids = vec![Ids::admin()];
    account_ids.extend(accounts);
    let message = public_transaction::Message::try_new(
        Ids::stablecoin_program(),
        account_ids,
        vec![current_nonce(state, Ids::admin())],
        instruction,
    )
    .expect("valid poke message");
    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::admin()]);
    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, block_id, now)
}

fn read_accumulator(state: &V03State) -> stablecoin_core::StabilityFeeAccumulator {
    stablecoin_core::StabilityFeeAccumulator::try_from(
        &state
            .get_account_by_id(compute_stability_fee_accumulator_pda(
                Ids::stablecoin_program(),
            ))
            .data,
    )
    .expect("valid StabilityFeeAccumulator")
}

fn read_redemption_price_state(state: &V03State) -> stablecoin_core::RedemptionPriceState {
    stablecoin_core::RedemptionPriceState::try_from(
        &state
            .get_account_by_id(compute_redemption_price_state_pda(Ids::stablecoin_program()))
            .data,
    )
    .expect("valid RedemptionPriceState")
}

#[test]
fn stablecoin_initialize_program_creates_globals_and_stablecoin_definition() {
    use stablecoin_core::math::FIXED_POINT_ONE;

    let now: u64 = 1_700_000_000;
    let state = initialize_protocol(now, 0);

    // ProtocolParameters claimed with the expected handles.
    let pp = stablecoin_core::ProtocolParameters::try_from(
        &state
            .get_account_by_id(compute_protocol_parameters_pda(Ids::stablecoin_program()))
            .data,
    )
    .expect("valid ProtocolParameters");
    assert_eq!(pp.admin_account_id, Ids::admin());
    assert_eq!(pp.freeze_authority_account_id, Ids::freeze_authority());
    assert_eq!(
        pp.stablecoin_definition_id,
        Ids::stablecoin_definition_pda()
    );
    assert_eq!(pp.collateral_definition_id, Ids::collateral_definition());
    assert_eq!(pp.market_price_oracle_id, Ids::oracle());
    assert!(!pp.is_frozen);

    // Accumulator anchored at FIXED_POINT_ONE / now.
    let acc = stablecoin_core::StabilityFeeAccumulator::try_from(
        &state
            .get_account_by_id(compute_stability_fee_accumulator_pda(
                Ids::stablecoin_program(),
            ))
            .data,
    )
    .expect("valid StabilityFeeAccumulator");
    assert_eq!(acc.accumulated_rate_at_last_accrual, FIXED_POINT_ONE);
    assert_eq!(acc.last_accrued_at, now);

    // Redemption price anchored at the initial value / now.
    let rp = stablecoin_core::RedemptionPriceState::try_from(
        &state
            .get_account_by_id(compute_redemption_price_state_pda(Ids::stablecoin_program()))
            .data,
    )
    .expect("valid RedemptionPriceState");
    assert_eq!(rp.redemption_price_at_last_update, FIXED_POINT_ONE / 2);
    assert_eq!(rp.redemption_rate_per_millisecond, FIXED_POINT_ONE);
    assert_eq!(rp.controller_integral_term, 0);
    assert_eq!(rp.last_updated_at, now);

    // Stablecoin definition created via the chained Token::NewFungibleDefinition.
    let definition = TokenDefinition::try_from(
        &state
            .get_account_by_id(Ids::stablecoin_definition_pda())
            .data,
    )
    .expect("valid TokenDefinition");
    match definition {
        TokenDefinition::Fungible {
            name,
            total_supply,
            metadata_id,
            authority,
        } => {
            assert_eq!(name, "test-stable");
            assert_eq!(total_supply, 0);
            assert_eq!(metadata_id, None);
            // Self/PDA authority: the definition is its own mint authority, so later
            // debt operations can mint/burn by presenting the definition PDA seed.
            assert_eq!(authority, Some(Ids::stablecoin_definition_pda()));
        }
        TokenDefinition::NonFungible { .. } => panic!("expected Fungible definition"),
    }

    // Empty master holding created alongside the definition.
    let master = TokenHolding::try_from(
        &state
            .get_account_by_id(compute_stablecoin_master_holding_pda(
                Ids::stablecoin_program(),
            ))
            .data,
    )
    .expect("valid TokenHolding");
    match master {
        TokenHolding::Fungible {
            definition_id,
            balance,
        } => {
            assert_eq!(definition_id, Ids::stablecoin_definition_pda());
            assert_eq!(balance, 0);
        }
        TokenHolding::NftMaster { .. } | TokenHolding::NftPrintedCopy { .. } => {
            panic!("expected Fungible holding")
        }
    }
}

#[test]
fn stablecoin_accrue_stability_fee_advances_accumulator() {
    use stablecoin_core::math::FIXED_POINT_ONE;

    let start: u64 = 1_700_000_000_000;
    let mut state = initialize_protocol(start, 0);

    let before = read_accumulator(&state);
    assert_eq!(before.accumulated_rate_at_last_accrual, FIXED_POINT_ONE);
    assert_eq!(before.last_accrued_at, start);

    // Accrual has no throttle, so any positive delta works; one hour, in ms.
    let now = start + 3_600_000;
    submit_poke(
        &mut state,
        now,
        2,
        stablecoin_core::Instruction::AccrueStabilityFee,
        vec![
            compute_protocol_parameters_pda(Ids::stablecoin_program()),
            compute_stability_fee_accumulator_pda(Ids::stablecoin_program()),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
    )
    .expect("accrue_stability_fee must succeed");

    let after = read_accumulator(&state);
    assert_eq!(
        after.accumulated_rate_at_last_accrual,
        stablecoin_core::math::compute_current_accumulated_rate(
            FIXED_POINT_ONE,
            protocol_config::STABILITY_FEE_PER_MILLISECOND,
            start,
            now,
        ),
    );
    assert!(after.accumulated_rate_at_last_accrual > FIXED_POINT_ONE);
    assert_eq!(after.last_accrued_at, now);
}

#[test]
fn stablecoin_update_redemption_rate_drifts_redemption_price() {
    use stablecoin_core::math::FIXED_POINT_ONE;

    let start: u64 = 1_700_000_000_000;
    // A live proportional gain, otherwise the rate would stay pinned at 1.0.
    let mut state = initialize_protocol(
        start,
        i128::try_from(FIXED_POINT_ONE).expect("FIXED_POINT_ONE fits i128"),
    );

    // Ten minutes later — past the 300_000 ms minimum interval.
    let now = start + 600_000;

    // A fresh observation well below the 0.5 redemption target, so
    // error = redemption − market > 0.
    state.force_insert_account(
        Ids::oracle(),
        Accounts::oracle_with(
            Ids::stablecoin_definition_pda(),
            Ids::collateral_definition(),
            FIXED_POINT_ONE / 4,
            now,
        ),
    );

    submit_poke(
        &mut state,
        now,
        2,
        stablecoin_core::Instruction::UpdateRedemptionRate,
        vec![
            compute_protocol_parameters_pda(Ids::stablecoin_program()),
            compute_redemption_price_state_pda(Ids::stablecoin_program()),
            Ids::oracle(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
    )
    .expect("update_redemption_rate must succeed");

    let after = read_redemption_price_state(&state);
    // Positive error with a positive Kp drives the rate ABOVE 1.0: the redemption
    // price rises, pulling the market up toward it (negative feedback, no negation).
    assert!(after.redemption_rate_per_millisecond > FIXED_POINT_ONE);
    // The rate change is capped per update by RATE_DELTA_CLAMP.
    assert_eq!(
        after.redemption_rate_per_millisecond,
        FIXED_POINT_ONE
            + u128::try_from(stablecoin_core::RATE_DELTA_CLAMP)
                .expect("RATE_DELTA_CLAMP is positive"),
    );
    // Re-anchored at the price projected from the OLD rate (exactly 1.0, so the
    // anchor is unchanged) and stamped with now.
    assert_eq!(
        after.redemption_price_at_last_update,
        protocol_config::INITIAL_REDEMPTION_PRICE
    );
    assert_eq!(after.last_updated_at, now);
}

#[test]
fn stablecoin_refresh_globals_advances_both_then_fee_only_when_oracle_stale() {
    use stablecoin_core::math::FIXED_POINT_ONE;

    let start: u64 = 1_700_000_000_000;
    let mut state = initialize_protocol(
        start,
        i128::try_from(FIXED_POINT_ONE).expect("FIXED_POINT_ONE fits i128"),
    );

    let refresh_accounts = || {
        vec![
            compute_protocol_parameters_pda(Ids::stablecoin_program()),
            compute_stability_fee_accumulator_pda(Ids::stablecoin_program()),
            compute_redemption_price_state_pda(Ids::stablecoin_program()),
            Ids::oracle(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ]
    };

    // --- Pass 1: fresh oracle, interval due => BOTH halves run. ---
    let first = start + 600_000;
    state.force_insert_account(
        Ids::oracle(),
        Accounts::oracle_with(
            Ids::stablecoin_definition_pda(),
            Ids::collateral_definition(),
            FIXED_POINT_ONE / 4,
            first,
        ),
    );

    submit_poke(
        &mut state,
        first,
        2,
        stablecoin_core::Instruction::RefreshGlobals,
        refresh_accounts(),
    )
    .expect("refresh_globals must advance both halves");

    let accumulator_after_first = read_accumulator(&state);
    let redemption_after_first = read_redemption_price_state(&state);
    assert_eq!(accumulator_after_first.last_accrued_at, first);
    assert!(accumulator_after_first.accumulated_rate_at_last_accrual > FIXED_POINT_ONE);
    assert!(redemption_after_first.redemption_rate_per_millisecond > FIXED_POINT_ONE);
    assert_eq!(redemption_after_first.last_updated_at, first);

    // --- Pass 2: the oracle observation is left where it was and the clock jumps
    // well past the max age => only the fee half runs, and the call still succeeds. ---
    let second = first + 2_000_000; // > the 900_000 ms maximum oracle age
    submit_poke(
        &mut state,
        second,
        3,
        stablecoin_core::Instruction::RefreshGlobals,
        refresh_accounts(),
    )
    .expect("refresh_globals must still succeed with a stale oracle");

    let accumulator_after_second = read_accumulator(&state);
    let redemption_after_second = read_redemption_price_state(&state);
    // Fee half ran again.
    assert_eq!(accumulator_after_second.last_accrued_at, second);
    assert!(
        accumulator_after_second.accumulated_rate_at_last_accrual
            > accumulator_after_first.accumulated_rate_at_last_accrual
    );
    // Redemption half skipped without panicking — byte-identical to pass 1.
    assert_eq!(redemption_after_second, redemption_after_first);
}
