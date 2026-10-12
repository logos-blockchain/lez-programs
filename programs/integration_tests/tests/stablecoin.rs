use std::collections::HashMap;

use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use integration_tests::{
    private_authorized_init_identity, private_authorized_update_identity,
    private_foreign_init_identity, GroupOwner,
};
use lee::{
    error::LeeError,
    execute_and_prove,
    privacy_preserving_transaction::{
        circuit::{ProgramWithDependencies, ProvingInput},
        Message, PrivacyPreservingTransaction, WitnessSet,
    },
    program::Program,
    public_transaction, PrivateKey, PublicKey, PublicTransaction, V03State,
};
use lee_core::{
    account::{Account, AccountId, Nonce, ProgramShardSelector, ShardData},
    encryption::ViewingPublicKey,
    AuthorizationSecretKey, Commitment, Identifier, Nullifier, NullifierPublicKey,
    NullifierSecretKey,
};
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
struct PrivateKeys;

impl PrivateKeys {
    /// The authorization key is the root credential under v0.2.5: the circuit takes an
    /// `ask` and derives the `nsk` from it, so the fixture roots here.
    fn destination_ask() -> AuthorizationSecretKey {
        AuthorizationSecretKey([111; 32])
    }

    fn destination_nsk() -> NullifierSecretKey {
        NullifierSecretKey::from(&Self::destination_ask())
    }

    fn destination_npk() -> NullifierPublicKey {
        NullifierPublicKey::from(&Self::destination_nsk())
    }

    fn destination_vpk() -> ViewingPublicKey {
        ViewingPublicKey::from_seed(&[141; 32], &[142; 32])
    }

    fn destination_id() -> AccountId {
        AccountId::for_regular_private_account(
            &Self::destination_npk(),
            &Self::destination_vpk(),
            Identifier::ZERO,
        )
    }

    /// The authorization key is the root credential under v0.2.5: the circuit takes an
    /// `ask` and derives the `nsk` from it, so the fixture roots here.
    fn stablecoin_holding_ask() -> AuthorizationSecretKey {
        AuthorizationSecretKey([121; 32])
    }

    fn stablecoin_holding_nsk() -> NullifierSecretKey {
        NullifierSecretKey::from(&Self::stablecoin_holding_ask())
    }

    fn stablecoin_holding_npk() -> NullifierPublicKey {
        NullifierPublicKey::from(&Self::stablecoin_holding_nsk())
    }

    fn stablecoin_holding_vpk() -> ViewingPublicKey {
        ViewingPublicKey::from_seed(&[151; 32], &[152; 32])
    }

    fn stablecoin_holding_id() -> AccountId {
        AccountId::for_regular_private_account(
            &Self::stablecoin_holding_npk(),
            &Self::stablecoin_holding_vpk(),
            Identifier::ZERO,
        )
    }
}

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

    fn freeze_authority() -> PrivateKey {
        PrivateKey::try_new([45; 32]).expect("valid private key")
    }
}

impl Ids {
    /// The program's account id: since v0.2.5 a program is addressed by its deployed
    /// `ProgramHeader` account, and the test harness seeds that header at the ImageID
    /// bijection address, so `AccountId::from(<ELF>_ID)` is where it lives here.
    fn token_program() -> AccountId {
        AccountId::from_builtin_program(token_methods::TOKEN_ID)
    }

    /// The program's account id: since v0.2.5 a program is addressed by its deployed
    /// `ProgramHeader` account, and the test harness seeds that header at the ImageID
    /// bijection address, so `AccountId::from(<ELF>_ID)` is where it lives here.
    fn stablecoin_program() -> AccountId {
        AccountId::from_builtin_program(stablecoin_methods::STABLECOIN_ID)
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
        AccountId::from(&PublicKey::new_from_private_key(&Keys::freeze_authority()))
    }

    /// The program that writes the market-price oracle's shard. v0.2.5 only
    /// needed a non-default owner here; v0.3.0 pins it — the oracle slot's
    /// `program_account_id` must equal
    /// `ProtocolParameters::market_price_oracle_program_id`.
    fn oracle_program() -> AccountId {
        AccountId::new([9u8; 32])
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

    fn collateral_top_up() -> u128 {
        100_000
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
        Account::default().with_shard(
            Ids::token_program(),
            ShardData::from(&TokenDefinition::Fungible {
                name: String::from("Gold"),
                total_supply: Balances::user_holding_init(),
                metadata_id: None,
                authority: None,
            }),
        )
    }

    fn user_holding_init() -> Account {
        Account::default().with_shard(
            Ids::token_program(),
            ShardData::from(&TokenHolding::Fungible {
                definition_id: Ids::collateral_definition(),
                balance: Balances::user_holding_init(),
            }),
        )
    }

    fn stablecoin_definition_init() -> Account {
        Account::default().with_shard(
            Ids::token_program(),
            ShardData::from(&TokenDefinition::Fungible {
                name: String::from("DAI"),
                total_supply: Balances::stablecoin_supply_init(),
                metadata_id: None,
                authority: None,
            }),
        )
    }

    fn user_stablecoin_holding_init() -> Account {
        Account::default().with_shard(
            Ids::token_program(),
            ShardData::from(&TokenHolding::Fungible {
                definition_id: Ids::stablecoin_definition(),
                balance: Balances::user_stablecoin_holding_init(),
            }),
        )
    }

    fn position_with_debt_init() -> Account {
        Account::default().with_shard(
            Ids::stablecoin_program(),
            ShardData::from(&Position {
                owner_account_id: Ids::owner(),
                position_nonce: Ids::position_nonce(),
                vault_account_id: Ids::vault(),
                collateral_amount: Balances::collateral_deposit(),
                normalized_debt_amount: Balances::initial_debt(),
                opened_at: 0,
            }),
        )
    }

    /// A bootstrapped `ProtocolParameters`, force-inserted rather than produced by
    /// `initialize_program`, so the position tests stay independent of the bootstrap
    /// flow. The full lifecycle test in Plan 3 issue 08 uses the real bootstrap.
    fn protocol_parameters_init() -> Account {
        Account::default().with_shard(
            Ids::stablecoin_program(),
            ShardData::from(&stablecoin_core::ProtocolParameters {
                admin_account_id: Ids::admin(),
                freeze_authority_account_id: Ids::freeze_authority(),
                stablecoin_definition_id: Ids::stablecoin_definition(),
                collateral_definition_id: Ids::collateral_definition(),
                market_price_oracle_id: Ids::oracle(),
                market_price_oracle_program_id: Ids::oracle_program(),
                token_program_id: Ids::token_program(),
                stability_fee_per_millisecond: protocol_config::STABILITY_FEE_PER_MILLISECOND,
                controller_proportional_gain: 0,
                controller_integral_gain: 0,
                minimum_collateralization_ratio: stablecoin_core::math::FIXED_POINT_ONE * 3 / 2,
                minimum_milliseconds_between_rate_updates:
                    protocol_config::MINIMUM_MILLISECONDS_BETWEEN_RATE_UPDATES,
                maximum_oracle_price_age_milliseconds:
                    protocol_config::MAXIMUM_ORACLE_PRICE_AGE_MILLISECONDS,
                is_frozen: false,
            }),
        )
    }

    fn stability_fee_accumulator_init() -> Account {
        Account::default().with_shard(
            Ids::stablecoin_program(),
            ShardData::from(&stablecoin_core::StabilityFeeAccumulator {
                accumulated_rate_at_last_accrual: stablecoin_core::math::FIXED_POINT_ONE,
                last_accrued_at: OPEN_POSITION_NOW,
            }),
        )
    }

    fn redemption_price_state_init() -> Account {
        Account::default().with_shard(
            Ids::stablecoin_program(),
            ShardData::from(&stablecoin_core::RedemptionPriceState {
                redemption_price_at_last_update: protocol_config::INITIAL_REDEMPTION_PRICE,
                redemption_rate_per_millisecond: stablecoin_core::math::FIXED_POINT_ONE,
                controller_integral_term: 0,
                last_updated_at: OPEN_POSITION_NOW,
            }),
        )
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
        Account::default().with_shard(
            Ids::oracle_program(),
            ShardData::from(&twap_oracle_core::OraclePriceAccount {
                base_asset,
                quote_asset,
                price,
                timestamp,
                source_id: Ids::oracle(),
                confidence_interval: 0,
            }),
        )
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
    // The timestamp lives in the Clock Program's shard of the clock data account,
    // which is the shard every clock guard selects.
    let clock_account = Account::default().with_shard(
        clock_core::clock_account_id(),
        ShardData::try_from(data).expect("clock account data fits"),
    );
    state.force_insert_account(CLOCK_01_PROGRAM_ACCOUNT_ID, clock_account);
}

// v0.2.5 deleted `ProgramDeploymentTransaction`; deployment is now the `program_loader`
// pseudo-program's WriteSegment/CreateHeader flow. `with_programs` seeds each program in the
// shape that flow produces — a loader-owned header plus its segment — without making every
// test drive a deployment. The header lands at `AccountId::from(program.id())`, so a program's
// account id stays derivable from its ELF here.
fn deploy_programs(state: &mut V03State) {
    *state = std::mem::take(state).with_programs([
        Program::new(token_methods::TOKEN_ELF.to_vec().into()).expect("valid token ELF"),
        Program::new(stablecoin_methods::STABLECOIN_ELF.to_vec().into())
            .expect("valid stablecoin ELF"),
    ]);
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
        // v0.2.5 used a non-default owner to keep this unchanged account in the
        // state diff; under shards a native balance is what makes it non-empty.
        Account::funded(1),
    );
    state.force_insert_account(
        compute_protocol_parameters_pda(Ids::stablecoin_program()),
        Accounts::protocol_parameters_init(),
    );
    state.force_insert_account(
        compute_stability_fee_accumulator_pda(Ids::stablecoin_program()),
        Accounts::stability_fee_accumulator_init(),
    );
    state.force_insert_account(
        compute_redemption_price_state_pda(Ids::stablecoin_program()),
        Accounts::redemption_price_state_init(),
    );
    seed_clock(&mut state, OPEN_POSITION_NOW);
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
    state.force_insert_account(
        compute_protocol_parameters_pda(Ids::stablecoin_program()),
        Accounts::protocol_parameters_init(),
    );
    state.force_insert_account(
        compute_stability_fee_accumulator_pda(Ids::stablecoin_program()),
        Accounts::stability_fee_accumulator_init(),
    );
    seed_clock(&mut state, OPEN_POSITION_NOW);
    state
}

fn assert_position(state: &V03State, expected_collateral: u128) {
    let position = Position::try_from(
        state
            .get_account_by_id(Ids::position())
            .data
            .shard(Ids::stablecoin_program()),
    )
    .expect("valid Position");
    assert_eq!(position.collateral_amount, expected_collateral);
    assert_eq!(position.normalized_debt_amount, 0);
    assert_eq!(position.vault_account_id, Ids::vault());
    assert_eq!(position.owner_account_id, Ids::owner());
    assert_eq!(position.position_nonce, Ids::position_nonce());
}

fn assert_fungible_balance(state: &V03State, account_id: AccountId, expected_balance: u128) {
    assert_fungible_balance_of(
        state,
        account_id,
        Ids::collateral_definition(),
        expected_balance,
    );
}

/// Same check, for holdings of a definition other than the collateral one (the
/// lifecycle test also asserts stablecoin balances).
fn assert_fungible_balance_of(
    state: &V03State,
    account_id: AccountId,
    expected_definition: AccountId,
    expected_balance: u128,
) {
    let holding = TokenHolding::try_from(
        state
            .get_account_by_id(account_id)
            .data
            .shard(Ids::token_program()),
    )
    .expect("valid TokenHolding");
    match holding {
        TokenHolding::Fungible {
            definition_id,
            balance,
        } => {
            assert_eq!(definition_id, expected_definition);
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
        parameters: declared_parameters(&state),
        now: declared_now(&state),
        position_nonce: Ids::position_nonce(),
        initial_collateral_amount: Balances::collateral_deposit(),
    };
    let message = public_transaction::Message::try_new(
        Ids::stablecoin_program(),
        vec![
            ours(Ids::owner()),
            ours(Ids::position()),
            token_shard(Ids::vault()),
            token_shard(Ids::user_holding()),
            token_shard(Ids::collateral_definition()),
            ours(compute_protocol_parameters_pda(Ids::stablecoin_program())),
            clock_shard(),
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

    // Top the position up with more collateral from the same user holding.
    let deposit = stablecoin_core::Instruction::DepositCollateral {
        expected: declared_identity(&state, Ids::position()),
        vault_balance: declared_vault_balance(&state, Ids::vault()),
        parameters: declared_parameters(&state),
        amount: Balances::collateral_top_up(),
    };
    let message = public_transaction::Message::try_new(
        Ids::stablecoin_program(),
        vec![
            ours(Ids::owner()),
            ours(Ids::position()),
            token_shard(Ids::vault()),
            token_shard(Ids::user_holding()),
            ours(compute_protocol_parameters_pda(Ids::stablecoin_program())),
        ],
        vec![
            current_nonce(&state, Ids::owner()),
            current_nonce(&state, Ids::user_holding()),
        ],
        deposit,
    )
    .expect("valid deposit_collateral message");
    let witness_set = public_transaction::WitnessSet::for_message(
        &message,
        &[&Keys::owner(), &Keys::user_holding()],
    );
    let tx = PublicTransaction::new(message, witness_set);
    state
        .transition_from_public_transaction(&tx, 0, 0)
        .expect("deposit_collateral must succeed");

    let after_top_up = Balances::collateral_deposit() + Balances::collateral_top_up();
    assert_position(&state, after_top_up);
    assert_fungible_balance(&state, Ids::vault(), after_top_up);
    assert_fungible_balance(
        &state,
        Ids::user_holding(),
        Balances::user_holding_init() - after_top_up,
    );

    // Withdraw part of the collateral back to the same user holding.
    let withdraw = stablecoin_core::Instruction::WithdrawCollateral {
        expected: declared_identity(&state, Ids::position()),
        parameters: declared_parameters(&state),
        accumulator: declared_accumulator(&state),
        redemption: declared_redemption(&state),
        now: declared_now(&state),
        amount: Balances::collateral_withdraw(),
    };
    let message = public_transaction::Message::try_new(
        Ids::stablecoin_program(),
        vec![
            ours(Ids::owner()),
            ours(Ids::position()),
            token_shard(Ids::vault()),
            token_shard(Ids::user_holding()),
            ours(compute_stability_fee_accumulator_pda(
                Ids::stablecoin_program(),
            )),
            ours(compute_redemption_price_state_pda(Ids::stablecoin_program())),
            ours(compute_protocol_parameters_pda(Ids::stablecoin_program())),
            clock_shard(),
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

    let after_withdraw = after_top_up - Balances::collateral_withdraw();
    assert_position(&state, after_withdraw);
    assert_fungible_balance(&state, Ids::vault(), after_withdraw);
    assert_fungible_balance(
        &state,
        Ids::user_holding(),
        Balances::user_holding_init() - after_withdraw,
    );
}

#[test]
fn stablecoin_repay_debt_burns_stablecoins_and_decreases_debt() {
    let mut state = state_for_stablecoin_repay_tests();

    let repay = stablecoin_core::Instruction::RepayDebt {
        expected: declared_identity(&state, Ids::position()),
        parameters: declared_parameters(&state),
        accumulator: declared_accumulator(&state),
        now: declared_now(&state),
        amount: Balances::debt_repay_amount(),
    };
    let message = public_transaction::Message::try_new(
        Ids::stablecoin_program(),
        vec![
            ours(Ids::owner()),
            ours(Ids::position()),
            token_shard(Ids::stablecoin_definition()),
            token_shard(Ids::user_stablecoin_holding()),
            ours(compute_stability_fee_accumulator_pda(
                Ids::stablecoin_program(),
            )),
            ours(compute_protocol_parameters_pda(Ids::stablecoin_program())),
            clock_shard(),
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
    let position = Position::try_from(
        state
            .get_account_by_id(Ids::position())
            .data
            .shard(Ids::stablecoin_program()),
    )
    .expect("valid Position");
    assert_eq!(
        position.normalized_debt_amount,
        Balances::initial_debt() - Balances::debt_repay_amount()
    );
    assert_eq!(position.collateral_amount, Balances::collateral_deposit());

    // Stablecoin total supply decreased by the burn amount.
    let definition = TokenDefinition::try_from(
        state
            .get_account_by_id(Ids::stablecoin_definition())
            .data
            .shard(Ids::token_program()),
    )
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
    let holding = TokenHolding::try_from(
        state
            .get_account_by_id(Ids::user_stablecoin_holding())
            .data
            .shard(Ids::token_program()),
    )
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

/// Seeds the three read-only globals and the clock that the fee-aware position
/// instructions read. The privacy tests build their state by hand rather than via
/// `initialize_program`, so they need these inserted explicitly.
fn seed_stablecoin_globals(state: &mut V03State) {
    state.force_insert_account(
        compute_protocol_parameters_pda(Ids::stablecoin_program()),
        Accounts::protocol_parameters_init(),
    );
    state.force_insert_account(
        compute_stability_fee_accumulator_pda(Ids::stablecoin_program()),
        Accounts::stability_fee_accumulator_init(),
    );
    state.force_insert_account(
        compute_redemption_price_state_pda(Ids::stablecoin_program()),
        Accounts::redemption_price_state_init(),
    );
    seed_clock(state, OPEN_POSITION_NOW);
}

// ── declared values ────────────────────────────────────────────────────────
//
// Under v0.3.0 a plan sees only account metadata, so every protocol fact an
// instruction needs is declared in the instruction and pinned by a guard on the
// shard it came from. These helpers read the facts out of the seeded state,
// which is exactly what a real client does: the client's read makes the
// declaration, the program's guard makes it true.

fn declared_parameters(state: &V03State) -> stablecoin_core::ProtocolParameters {
    stablecoin_core::ProtocolParameters::try_from(
        state
            .get_account_by_id(compute_protocol_parameters_pda(Ids::stablecoin_program()))
            .data
            .shard(Ids::stablecoin_program()),
    )
    .expect("seeded ProtocolParameters must decode")
}

fn declared_accumulator(state: &V03State) -> stablecoin_core::StabilityFeeAccumulator {
    stablecoin_core::StabilityFeeAccumulator::try_from(
        state
            .get_account_by_id(compute_stability_fee_accumulator_pda(
                Ids::stablecoin_program(),
            ))
            .data
            .shard(Ids::stablecoin_program()),
    )
    .expect("seeded StabilityFeeAccumulator must decode")
}

fn declared_redemption(state: &V03State) -> stablecoin_core::RedemptionPriceState {
    stablecoin_core::RedemptionPriceState::try_from(
        state
            .get_account_by_id(compute_redemption_price_state_pda(Ids::stablecoin_program()))
            .data
            .shard(Ids::stablecoin_program()),
    )
    .expect("seeded RedemptionPriceState must decode")
}

/// The stored position's identity — `(owner, nonce, vault)`. Declaring it is how
/// a plan that cannot read the position still pins which position it means.
fn declared_identity(
    state: &V03State,
    position_id: AccountId,
) -> stablecoin_core::PositionIdentity {
    let position = Position::try_from(
        state
            .get_account_by_id(position_id)
            .data
            .shard(Ids::stablecoin_program()),
    )
    .expect("seeded Position must decode");
    stablecoin_core::PositionIdentity::from(&position)
}

/// A vault's collateral balance, read from the Token Program's shard.
fn declared_vault_balance(state: &V03State, vault_id: AccountId) -> u128 {
    match TokenHolding::try_from(
        state
            .get_account_by_id(vault_id)
            .data
            .shard(Ids::token_program()),
    )
    .expect("seeded vault holding must decode")
    {
        TokenHolding::Fungible { balance, .. } => balance,
        TokenHolding::NftMaster { .. } | TokenHolding::NftPrintedCopy { .. } => {
            panic!("a collateral vault must be a fungible holding")
        }
    }
}

/// The oracle observation a caller would quote: price and its timestamp.
fn declared_oracle(state: &V03State) -> (u128, u64) {
    let account = twap_oracle_core::OraclePriceAccount::try_from(
        state
            .get_account_by_id(Ids::oracle())
            .data
            .shard(Ids::oracle_program()),
    )
    .expect("seeded oracle observation must decode");
    (account.price, account.timestamp)
}

/// Wall-clock milliseconds from the seeded `CLOCK_01` shard.
fn declared_now(state: &V03State) -> u64 {
    ClockAccountData::from_bytes(
        state
            .get_account_by_id(CLOCK_01_PROGRAM_ACCOUNT_ID)
            .data
            .shard(clock_core::clock_account_id())
            .as_ref(),
    )
    .timestamp
}

/// One of this program's own shards.
fn ours(account_id: AccountId) -> ProgramShardSelector {
    ProgramShardSelector::new(account_id, Ids::stablecoin_program())
}

/// A Token Program shard — holdings, vaults and definitions.
fn token_shard(account_id: AccountId) -> ProgramShardSelector {
    ProgramShardSelector::new(account_id, Ids::token_program())
}

/// The Clock Program's shard of the canonical `CLOCK_01` data account, which is
/// the shard every clock guard selects.
fn clock_shard() -> ProgramShardSelector {
    ProgramShardSelector::new(CLOCK_01_PROGRAM_ACCOUNT_ID, clock_core::clock_account_id())
}

/// The four trailing public inputs of `WithdrawCollateral`, in ABI order.
fn withdraw_global_slots() -> Vec<ProgramShardSelector> {
    vec![
        ours(compute_stability_fee_accumulator_pda(
            Ids::stablecoin_program(),
        )),
        ours(compute_redemption_price_state_pda(Ids::stablecoin_program())),
        ours(compute_protocol_parameters_pda(Ids::stablecoin_program())),
        clock_shard(),
    ]
}

/// The three trailing public inputs of `RepayDebt`, in ABI order.
fn repay_global_slots() -> Vec<ProgramShardSelector> {
    vec![
        ours(compute_stability_fee_accumulator_pda(
            Ids::stablecoin_program(),
        )),
        ours(compute_protocol_parameters_pda(Ids::stablecoin_program())),
        clock_shard(),
    ]
}

fn stablecoin_program() -> Program {
    Program::new(stablecoin_methods::STABLECOIN_ELF.to_vec().into()).expect("valid stablecoin ELF")
}

fn token_program_instance() -> Program {
    Program::new(token_methods::TOKEN_ELF.to_vec().into()).expect("valid token ELF")
}

fn stablecoin_with_token_deps() -> ProgramWithDependencies {
    ProgramWithDependencies::new(
        stablecoin_program(),
        Ids::stablecoin_program(),
        HashMap::from([(Ids::token_program(), token_program_instance())]),
    )
}

/// `OpenPosition` through the privacy-preserving circuit, which LEZ v0.2.5 makes expressible.
///
/// Under v0.2.4 this was rejected with "Inconsistent authorization for account": the vault is
/// touched by two chained calls, and the caller had to hand each one a pre-state carrying its
/// own `is_authorized` flag — the second occurrence declared `false` after the first had been
/// authorized by a `pda_seeds` match. v0.2.5's chained calls carry bare account ids and the
/// runtime resolves both the state and the authorization itself, so the contradiction the
/// caller used to be able to state no longer exists.
#[test]
fn stablecoin_open_position_via_privacy_transaction() {
    let mut state = V03State::new();
    deploy_programs(&mut state);
    state.force_insert_account(
        Ids::collateral_definition(),
        Accounts::collateral_definition_init(),
    );
    state.force_insert_account(Ids::user_holding(), Accounts::user_holding_init());
    // The rebuilt `open_position` reads the global parameters and the clock before it
    // reaches the chained calls, so both must exist for the circuit to get as far as
    // the rejection this test is about.
    let parameters_id = compute_protocol_parameters_pda(Ids::stablecoin_program());
    state.force_insert_account(parameters_id, Accounts::protocol_parameters_init());
    seed_clock(&mut state, OPEN_POSITION_NOW);

    let owner_id = Ids::owner();
    let position_id =
        compute_position_pda(Ids::stablecoin_program(), owner_id, Ids::position_nonce());
    let vault_id = compute_position_vault_pda(Ids::stablecoin_program(), position_id);

    let instruction = stablecoin_core::Instruction::OpenPosition {
        parameters: declared_parameters(&state),
        now: declared_now(&state),
        position_nonce: Ids::position_nonce(),
        initial_collateral_amount: Balances::collateral_deposit(),
    };

    let result = execute_and_prove(
        ProvingInput {
            shard_selectors: vec![
                ours(owner_id),
                ours(position_id),
                token_shard(vault_id),
                token_shard(Ids::user_holding()),
                token_shard(Ids::collateral_definition()),
                ours(parameters_id),
                clock_shard(),
            ],
            signers: [owner_id, Ids::user_holding()].into(),
            private_witnesses: vec![],
            instruction_data: Program::serialize_instruction(instruction).unwrap(),
            ..Default::default()
        },
        &stablecoin_with_token_deps(),
    );

    let (output, _proof) =
        result.expect("v0.2.5 resolves the vault's two chained-call occurrences consistently");

    // Stop at the circuit: every account here is public, and a privacy-preserving transaction
    // needs at least one private action to be submittable. What this test pins is the circuit's
    // handling of an account touched by two sibling chained calls, which is exactly what used
    // to fail. Applying OpenPosition end to end is covered by
    // `stablecoin_open_position_then_withdraw_collateral` on the public path.
    let vault_action = output
        .public_actions
        .iter()
        .find(|action| action.account_id == vault_id)
        .expect("the vault must appear in the circuit output");
    assert!(
        vault_action
            .effects
            .iter()
            .any(|effect| effect.shard_program_account_id == Ids::token_program()),
        "the chained InitializeAccount must write the vault's Token Program shard"
    );
}

/// `WithdrawCollateral` to private account (`PrivateAuthorized`; `nsk` is known).
#[test]
fn stablecoin_withdraw_collateral_private_destination() {
    let mut state = V03State::new();
    deploy_programs(&mut state);
    seed_stablecoin_globals(&mut state);
    state.force_insert_account(
        Ids::collateral_definition(),
        Accounts::collateral_definition_init(),
    );

    let owner_id = Ids::owner();
    let position_id =
        compute_position_pda(Ids::stablecoin_program(), owner_id, Ids::position_nonce());
    let vault_id = compute_position_vault_pda(Ids::stablecoin_program(), position_id);

    let position_collateral = 500_000_u128;
    let withdraw_amount = 200_000_u128;

    let position_account = Account::default().with_shard(
        Ids::stablecoin_program(),
        ShardData::from(&Position {
            owner_account_id: owner_id,
            position_nonce: Ids::position_nonce(),
            vault_account_id: vault_id,
            collateral_amount: position_collateral,
            normalized_debt_amount: 0,
            opened_at: 0,
        }),
    );
    let vault_account = Account::default().with_shard(
        Ids::token_program(),
        ShardData::from(&TokenHolding::Fungible {
            definition_id: Ids::collateral_definition(),
            balance: position_collateral,
        }),
    );
    state.force_insert_account(position_id, position_account);
    state.force_insert_account(vault_id, vault_account);

    let destination_ask = PrivateKeys::destination_ask();
    let destination_nsk = NullifierSecretKey::from(&destination_ask);
    let destination_vpk = PrivateKeys::destination_vpk();
    let destination_id = PrivateKeys::destination_id();
    let destination_initial_balance = 100_000_u128;
    let destination_account = Account {
        nonce: Nonce::private_account_nonce_init(&destination_id),
        ..Account::default()
    }
    .with_shard(
        Ids::token_program(),
        ShardData::from(&TokenHolding::Fungible {
            definition_id: Ids::collateral_definition(),
            balance: destination_initial_balance,
        }),
    );
    state = state.with_private_accounts([(
        Commitment::new(&destination_id, &destination_account),
        Nullifier::for_account_initialization(&destination_id),
    )]);
    let membership_proof = state
        .get_proof_for_commitment(&Commitment::new(&destination_id, &destination_account))
        .expect("destination's commitment must be in the set");

    let instruction = stablecoin_core::Instruction::WithdrawCollateral {
        expected: declared_identity(&state, position_id),
        parameters: declared_parameters(&state),
        accumulator: declared_accumulator(&state),
        redemption: declared_redemption(&state),
        now: declared_now(&state),
        amount: withdraw_amount,
    };

    let (output, proof) = execute_and_prove(
        ProvingInput {
            shard_selectors: [
                vec![
                    ours(owner_id),
                    ours(position_id),
                    token_shard(vault_id),
                    token_shard(destination_id),
                ],
                withdraw_global_slots(),
            ]
            .concat(),
            signers: [owner_id, destination_id].into(),
            private_witnesses: vec![private_authorized_update_identity(
                destination_ask,
                &destination_vpk,
                destination_account.clone(),
                membership_proof,
            )],
            instruction_data: Program::serialize_instruction(instruction).unwrap(),
            ..Default::default()
        },
        &stablecoin_with_token_deps(),
    )
    .unwrap();

    let message = Message::from_circuit_output(vec![Nonce(0)], output);
    let witness_set = WitnessSet::for_message(&message, proof, &[&Keys::owner()]);
    state
        .transition_from_privacy_preserving_transaction(
            &PrivacyPreservingTransaction::new(message, witness_set),
            0,
            0,
        )
        .unwrap();

    let position = Position::try_from(
        state
            .get_account_by_id(position_id)
            .data
            .shard(Ids::stablecoin_program()),
    )
    .expect("valid Position");
    assert_eq!(
        position.collateral_amount,
        position_collateral - withdraw_amount
    );
    assert_eq!(position.normalized_debt_amount, 0);

    match TokenHolding::try_from(
        state
            .get_account_by_id(vault_id)
            .data
            .shard(Ids::token_program()),
    )
    .expect("valid holding")
    {
        TokenHolding::Fungible { balance, .. } => {
            assert_eq!(balance, position_collateral - withdraw_amount);
        }
        TokenHolding::NftMaster { .. } | TokenHolding::NftPrintedCopy { .. } => {
            panic!("expected Fungible vault holding")
        }
    }

    let destination_nonce_after = Nonce::private_account_nonce_init(&destination_id)
        .private_account_nonce_increment(&destination_nsk);
    let new_destination_account = Account {
        nonce: destination_nonce_after,
        ..Account::default()
    }
    .with_shard(
        Ids::token_program(),
        ShardData::from(&TokenHolding::Fungible {
            definition_id: Ids::collateral_definition(),
            balance: destination_initial_balance + withdraw_amount,
        }),
    );
    assert!(state
        .get_proof_for_commitment(&Commitment::new(&destination_id, &new_destination_account))
        .is_some());
}

/// `WithdrawCollateral` blocks withdraws to private accounts (via private donations);
/// `PrivateUnauthorized` account initialization (e.g., `nsk` is not known) is not permitted
/// due to the assertion in `withdraw_collateral.rs` asserts `destination.account !=
/// Account::default()`
#[test]
fn stablecoin_withdraw_collateral_to_new_private_destination() {
    let mut state = V03State::new();
    deploy_programs(&mut state);
    seed_stablecoin_globals(&mut state);
    state.force_insert_account(
        Ids::collateral_definition(),
        Accounts::collateral_definition_init(),
    );

    let owner_id = Ids::owner();
    let position_id =
        compute_position_pda(Ids::stablecoin_program(), owner_id, Ids::position_nonce());
    let vault_id = compute_position_vault_pda(Ids::stablecoin_program(), position_id);

    let position_collateral = 500_000_u128;
    let withdraw_amount = 200_000_u128;

    let position_account = Account::default().with_shard(
        Ids::stablecoin_program(),
        ShardData::from(&Position {
            owner_account_id: owner_id,
            position_nonce: Ids::position_nonce(),
            vault_account_id: vault_id,
            collateral_amount: position_collateral,
            normalized_debt_amount: 0,
            opened_at: 0,
        }),
    );
    let vault_account = Account::default().with_shard(
        Ids::token_program(),
        ShardData::from(&TokenHolding::Fungible {
            definition_id: Ids::collateral_definition(),
            balance: position_collateral,
        }),
    );
    state.force_insert_account(position_id, position_account);
    state.force_insert_account(vault_id, vault_account);

    let destination_npk = PrivateKeys::destination_npk();
    let destination_vpk = PrivateKeys::destination_vpk();
    let destination_id = PrivateKeys::destination_id();

    let instruction = stablecoin_core::Instruction::WithdrawCollateral {
        expected: declared_identity(&state, position_id),
        parameters: declared_parameters(&state),
        accumulator: declared_accumulator(&state),
        redemption: declared_redemption(&state),
        now: declared_now(&state),
        amount: withdraw_amount,
    };

    let result = execute_and_prove(
        ProvingInput {
            shard_selectors: [
                vec![
                    ours(owner_id),
                    ours(position_id),
                    token_shard(vault_id),
                    token_shard(destination_id),
                ],
                withdraw_global_slots(),
            ]
            .concat(),
            signers: [owner_id].into(),
            private_witnesses: vec![private_foreign_init_identity(
                destination_npk,
                &destination_vpk,
                state.commitment_root(),
            )],
            instruction_data: Program::serialize_instruction(instruction).unwrap(),
            ..Default::default()
        },
        &stablecoin_with_token_deps(),
    );

    // v0.2.5 rejected this with "User collateral holding must be initialized".
    // v0.3.0 made it expressible: `plan_withdraw_collateral` no longer inspects the
    // destination at all — it chains a vault→user transfer, and the Token Program
    // will credit a shard that does not exist yet, because writing an empty shard
    // denies the address to nobody. The withdrawal therefore lands in a
    // brand-new private holding.
    let (output, _proof) =
        result.expect("withdrawing to a brand-new private destination is expressible now");
    assert_eq!(
        output.private_actions.len(),
        1,
        "the fresh private destination must receive exactly one private action"
    );
}

#[test]
fn stablecoin_withdraw_collateral_group_owned_destination() {
    let mut state = V03State::new();
    deploy_programs(&mut state);
    seed_stablecoin_globals(&mut state);
    state.force_insert_account(
        Ids::collateral_definition(),
        Accounts::collateral_definition_init(),
    );

    let owner_id = Ids::owner();
    let position_id =
        compute_position_pda(Ids::stablecoin_program(), owner_id, Ids::position_nonce());
    let vault_id = compute_position_vault_pda(Ids::stablecoin_program(), position_id);

    let position_collateral = 500_000_u128;
    let withdraw_amount = 200_000_u128;

    let position_account = Account::default().with_shard(
        Ids::stablecoin_program(),
        ShardData::from(&Position {
            owner_account_id: owner_id,
            position_nonce: Ids::position_nonce(),
            vault_account_id: vault_id,
            collateral_amount: position_collateral,
            normalized_debt_amount: 0,
            opened_at: 0,
        }),
    );
    let vault_account = Account::default().with_shard(
        Ids::token_program(),
        ShardData::from(&TokenHolding::Fungible {
            definition_id: Ids::collateral_definition(),
            balance: position_collateral,
        }),
    );
    state.force_insert_account(position_id, position_account);
    state.force_insert_account(vault_id, vault_account);

    // Alice creates the group and derives the shared destination's keys; Bob is admitted via
    // the real seal/unseal handshake and independently re-derives the same keys.
    let alice = GroupOwner::new([7_u8; 32]);
    let bob_ask = alice.admit_member();
    let bob_nsk = NullifierSecretKey::from(&bob_ask);
    let destination_vpk = alice.vpk;
    let destination_id = alice.id;

    let destination_initial_balance = 100_000_u128;
    let destination_account = Account {
        nonce: Nonce::private_account_nonce_init(&destination_id),
        ..Account::default()
    }
    .with_shard(
        Ids::token_program(),
        ShardData::from(&TokenHolding::Fungible {
            definition_id: Ids::collateral_definition(),
            balance: destination_initial_balance,
        }),
    );
    state = state.with_private_accounts([(
        Commitment::new(&destination_id, &destination_account),
        Nullifier::for_account_initialization(&destination_id),
    )]);
    let membership_proof = state
        .get_proof_for_commitment(&Commitment::new(&destination_id, &destination_account))
        .expect("destination's commitment must be in the set");

    let instruction = stablecoin_core::Instruction::WithdrawCollateral {
        expected: declared_identity(&state, position_id),
        parameters: declared_parameters(&state),
        accumulator: declared_accumulator(&state),
        redemption: declared_redemption(&state),
        now: declared_now(&state),
        amount: withdraw_amount,
    };

    let (output, proof) = execute_and_prove(
        ProvingInput {
            shard_selectors: [
                vec![
                    ours(owner_id),
                    ours(position_id),
                    token_shard(vault_id),
                    token_shard(destination_id),
                ],
                withdraw_global_slots(),
            ]
            .concat(),
            signers: [owner_id, destination_id].into(),
            private_witnesses: vec![private_authorized_update_identity(
                bob_ask,
                &destination_vpk,
                destination_account.clone(),
                membership_proof,
            )],
            instruction_data: Program::serialize_instruction(instruction).unwrap(),
            ..Default::default()
        },
        &stablecoin_with_token_deps(),
    )
    .unwrap();

    let message = Message::from_circuit_output(vec![Nonce(0)], output);
    let witness_set = WitnessSet::for_message(&message, proof, &[&Keys::owner()]);
    state
        .transition_from_privacy_preserving_transaction(
            &PrivacyPreservingTransaction::new(message, witness_set),
            0,
            0,
        )
        .unwrap();

    let position = Position::try_from(
        state
            .get_account_by_id(position_id)
            .data
            .shard(Ids::stablecoin_program()),
    )
    .expect("valid Position");
    assert_eq!(
        position.collateral_amount,
        position_collateral - withdraw_amount
    );

    let destination_nonce_after = Nonce::private_account_nonce_init(&destination_id)
        .private_account_nonce_increment(&bob_nsk);
    let new_destination_account = Account {
        nonce: destination_nonce_after,
        ..Account::default()
    }
    .with_shard(
        Ids::token_program(),
        ShardData::from(&TokenHolding::Fungible {
            definition_id: Ids::collateral_definition(),
            balance: destination_initial_balance + withdraw_amount,
        }),
    );
    assert!(state
        .get_proof_for_commitment(&Commitment::new(&destination_id, &new_destination_account))
        .is_some());
}

#[test]
fn stablecoin_repay_debt_private_stablecoin_holding() {
    let mut state = V03State::new();
    deploy_programs(&mut state);
    seed_stablecoin_globals(&mut state);
    state.force_insert_account(
        Ids::collateral_definition(),
        Accounts::collateral_definition_init(),
    );
    state.force_insert_account(
        Ids::stablecoin_definition(),
        Accounts::stablecoin_definition_init(),
    );

    let owner_id = Ids::owner();
    let position_id =
        compute_position_pda(Ids::stablecoin_program(), owner_id, Ids::position_nonce());
    let vault_id = compute_position_vault_pda(Ids::stablecoin_program(), position_id);

    let position_collateral = Balances::collateral_deposit();
    let initial_debt = Balances::initial_debt();
    let repay_amount = Balances::debt_repay_amount();

    let position_account = Account::default().with_shard(
        Ids::stablecoin_program(),
        ShardData::from(&Position {
            owner_account_id: owner_id,
            position_nonce: Ids::position_nonce(),
            vault_account_id: vault_id,
            collateral_amount: position_collateral,
            normalized_debt_amount: initial_debt,
            opened_at: 0,
        }),
    );
    state.force_insert_account(position_id, position_account);

    let stablecoin_holding_ask = PrivateKeys::stablecoin_holding_ask();
    let stablecoin_holding_nsk = NullifierSecretKey::from(&stablecoin_holding_ask);
    let stablecoin_holding_vpk = PrivateKeys::stablecoin_holding_vpk();
    let stablecoin_holding_id = PrivateKeys::stablecoin_holding_id();
    let initial_stablecoin_balance = Balances::user_stablecoin_holding_init();
    let stablecoin_holding_account = Account {
        nonce: Nonce::private_account_nonce_init(&stablecoin_holding_id),
        ..Account::default()
    }
    .with_shard(
        Ids::token_program(),
        ShardData::from(&TokenHolding::Fungible {
            definition_id: Ids::stablecoin_definition(),
            balance: initial_stablecoin_balance,
        }),
    );
    state = state.with_private_accounts([(
        Commitment::new(&stablecoin_holding_id, &stablecoin_holding_account),
        Nullifier::for_account_initialization(&stablecoin_holding_id),
    )]);
    let membership_proof = state
        .get_proof_for_commitment(&Commitment::new(
            &stablecoin_holding_id,
            &stablecoin_holding_account,
        ))
        .expect("stablecoin holding's commitment must be in the set");

    let instruction = stablecoin_core::Instruction::RepayDebt {
        expected: declared_identity(&state, position_id),
        parameters: declared_parameters(&state),
        accumulator: declared_accumulator(&state),
        now: declared_now(&state),
        amount: repay_amount,
    };

    let (output, proof) = execute_and_prove(
        ProvingInput {
            shard_selectors: [
                vec![
                    ours(owner_id),
                    ours(position_id),
                    token_shard(Ids::stablecoin_definition()),
                    token_shard(stablecoin_holding_id),
                ],
                repay_global_slots(),
            ]
            .concat(),
            signers: [owner_id, stablecoin_holding_id].into(),
            private_witnesses: vec![private_authorized_update_identity(
                stablecoin_holding_ask,
                &stablecoin_holding_vpk,
                stablecoin_holding_account.clone(),
                membership_proof,
            )],
            instruction_data: Program::serialize_instruction(instruction).unwrap(),
            ..Default::default()
        },
        &stablecoin_with_token_deps(),
    )
    .unwrap();

    let message = Message::from_circuit_output(vec![Nonce(0)], output);
    let witness_set = WitnessSet::for_message(&message, proof, &[&Keys::owner()]);
    state
        .transition_from_privacy_preserving_transaction(
            &PrivacyPreservingTransaction::new(message, witness_set),
            0,
            0,
        )
        .unwrap();

    let position = Position::try_from(
        state
            .get_account_by_id(position_id)
            .data
            .shard(Ids::stablecoin_program()),
    )
    .expect("valid Position");
    assert_eq!(position.normalized_debt_amount, initial_debt - repay_amount);
    assert_eq!(position.collateral_amount, position_collateral);

    match TokenDefinition::try_from(
        state
            .get_account_by_id(Ids::stablecoin_definition())
            .data
            .shard(Ids::token_program()),
    )
    .expect("valid TokenDefinition")
    {
        TokenDefinition::Fungible { total_supply, .. } => {
            assert_eq!(
                total_supply,
                Balances::stablecoin_supply_init() - repay_amount
            );
        }
        _ => panic!("expected Fungible definition"),
    }

    let stablecoin_holding_nonce_after = Nonce::private_account_nonce_init(&stablecoin_holding_id)
        .private_account_nonce_increment(&stablecoin_holding_nsk);
    let new_stablecoin_holding_account = Account {
        nonce: stablecoin_holding_nonce_after,
        ..Account::default()
    }
    .with_shard(
        Ids::token_program(),
        ShardData::from(&TokenHolding::Fungible {
            definition_id: Ids::stablecoin_definition(),
            balance: initial_stablecoin_balance - repay_amount,
        }),
    );
    assert!(state
        .get_proof_for_commitment(&Commitment::new(
            &stablecoin_holding_id,
            &new_stablecoin_holding_account
        ))
        .is_some());
}

#[test]
fn stablecoin_repay_debt_group_owned_stablecoin_holding() {
    let mut state = V03State::new();
    deploy_programs(&mut state);
    seed_stablecoin_globals(&mut state);
    state.force_insert_account(
        Ids::collateral_definition(),
        Accounts::collateral_definition_init(),
    );
    state.force_insert_account(
        Ids::stablecoin_definition(),
        Accounts::stablecoin_definition_init(),
    );

    let owner_id = Ids::owner();
    let position_id =
        compute_position_pda(Ids::stablecoin_program(), owner_id, Ids::position_nonce());
    let vault_id = compute_position_vault_pda(Ids::stablecoin_program(), position_id);

    let position_collateral = Balances::collateral_deposit();
    let initial_debt = Balances::initial_debt();
    let repay_amount = Balances::debt_repay_amount();

    let position_account = Account::default().with_shard(
        Ids::stablecoin_program(),
        ShardData::from(&Position {
            owner_account_id: owner_id,
            position_nonce: Ids::position_nonce(),
            vault_account_id: vault_id,
            collateral_amount: position_collateral,
            normalized_debt_amount: initial_debt,
            opened_at: 0,
        }),
    );
    state.force_insert_account(position_id, position_account);

    // Alice creates the group and derives the shared stablecoin holding's keys; Bob is
    // admitted via the real seal/unseal handshake and independently re-derives the same keys.
    let alice = GroupOwner::new([7_u8; 32]);
    let bob_ask = alice.admit_member();
    let bob_nsk = NullifierSecretKey::from(&bob_ask);
    let holding_vpk = alice.vpk;
    let holding_id = alice.id;

    let initial_stablecoin_balance = Balances::user_stablecoin_holding_init();
    let holding_account = Account {
        nonce: Nonce::private_account_nonce_init(&holding_id),
        ..Account::default()
    }
    .with_shard(
        Ids::token_program(),
        ShardData::from(&TokenHolding::Fungible {
            definition_id: Ids::stablecoin_definition(),
            balance: initial_stablecoin_balance,
        }),
    );
    state = state.with_private_accounts([(
        Commitment::new(&holding_id, &holding_account),
        Nullifier::for_account_initialization(&holding_id),
    )]);
    let membership_proof = state
        .get_proof_for_commitment(&Commitment::new(&holding_id, &holding_account))
        .expect("stablecoin holding's commitment must be in the set");

    let instruction = stablecoin_core::Instruction::RepayDebt {
        expected: declared_identity(&state, position_id),
        parameters: declared_parameters(&state),
        accumulator: declared_accumulator(&state),
        now: declared_now(&state),
        amount: repay_amount,
    };

    let (output, proof) = execute_and_prove(
        ProvingInput {
            shard_selectors: [
                vec![
                    ours(owner_id),
                    ours(position_id),
                    token_shard(Ids::stablecoin_definition()),
                    token_shard(holding_id),
                ],
                repay_global_slots(),
            ]
            .concat(),
            signers: [owner_id, holding_id].into(),
            private_witnesses: vec![private_authorized_update_identity(
                bob_ask,
                &holding_vpk,
                holding_account.clone(),
                membership_proof,
            )],
            instruction_data: Program::serialize_instruction(instruction).unwrap(),
            ..Default::default()
        },
        &stablecoin_with_token_deps(),
    )
    .unwrap();

    let message = Message::from_circuit_output(vec![Nonce(0)], output);
    let witness_set = WitnessSet::for_message(&message, proof, &[&Keys::owner()]);
    state
        .transition_from_privacy_preserving_transaction(
            &PrivacyPreservingTransaction::new(message, witness_set),
            0,
            0,
        )
        .unwrap();

    let position = Position::try_from(
        state
            .get_account_by_id(position_id)
            .data
            .shard(Ids::stablecoin_program()),
    )
    .expect("valid Position");
    assert_eq!(position.normalized_debt_amount, initial_debt - repay_amount);

    let holding_nonce_after =
        Nonce::private_account_nonce_init(&holding_id).private_account_nonce_increment(&bob_nsk);
    let new_holding_account = Account {
        nonce: holding_nonce_after,
        ..Account::default()
    }
    .with_shard(
        Ids::token_program(),
        ShardData::from(&TokenHolding::Fungible {
            definition_id: Ids::stablecoin_definition(),
            balance: initial_stablecoin_balance - repay_amount,
        }),
    );
    assert!(state
        .get_proof_for_commitment(&Commitment::new(&holding_id, &new_holding_account))
        .is_some());
}

#[test]
fn stablecoin_group_owned_position_owner() {
    let mut state = V03State::new();
    deploy_programs(&mut state);
    seed_stablecoin_globals(&mut state);
    state.force_insert_account(
        Ids::collateral_definition(),
        Accounts::collateral_definition_init(),
    );
    state.force_insert_account(Ids::user_holding(), Accounts::user_holding_init());

    // Alice creates the group and derives the shared owner identity's keys; Bob is admitted
    // via the real seal/unseal handshake and independently re-derives the same keys.
    let alice = GroupOwner::new([7_u8; 32]);
    let bob_ask = alice.admit_member();
    let owner_id = alice.id;

    // Position/vault addresses are derived from the group-owned owner_id — still ordinary
    // public PDAs (the seed formula doesn't care whether owner_id is public or private), seeded
    // directly since OpenPosition can't be routed through the privacy circuit at all.
    let position_id =
        compute_position_pda(Ids::stablecoin_program(), owner_id, Ids::position_nonce());
    let vault_id = compute_position_vault_pda(Ids::stablecoin_program(), position_id);

    let position_collateral = 500_000_u128;
    let withdraw_amount = 200_000_u128;
    let position_account = Account::default().with_shard(
        Ids::stablecoin_program(),
        ShardData::from(&Position {
            owner_account_id: owner_id,
            position_nonce: Ids::position_nonce(),
            vault_account_id: vault_id,
            collateral_amount: position_collateral,
            normalized_debt_amount: 0,
            opened_at: 0,
        }),
    );
    let vault_account = Account::default().with_shard(
        Ids::token_program(),
        ShardData::from(&TokenHolding::Fungible {
            definition_id: Ids::collateral_definition(),
            balance: position_collateral,
        }),
    );
    state.force_insert_account(position_id, position_account);
    state.force_insert_account(vault_id, vault_account);

    // Bob self-initializes and signs the owner identity in the same transaction, then
    // withdraws collateral through it. Destination stays public to isolate what's under test:
    // only the owner identity's privacy/sharing, nothing else.
    let instruction = stablecoin_core::Instruction::WithdrawCollateral {
        expected: declared_identity(&state, position_id),
        parameters: declared_parameters(&state),
        accumulator: declared_accumulator(&state),
        redemption: declared_redemption(&state),
        now: declared_now(&state),
        amount: withdraw_amount,
    };

    let (output, proof) = execute_and_prove(
        ProvingInput {
            shard_selectors: [
                vec![
                    ours(owner_id),
                    ours(position_id),
                    token_shard(vault_id),
                    token_shard(Ids::user_holding()),
                ],
                withdraw_global_slots(),
            ]
            .concat(),
            signers: [owner_id].into(),
            private_witnesses: vec![private_authorized_init_identity(
                bob_ask,
                &alice.vpk,
                state.commitment_root(),
            )],
            instruction_data: Program::serialize_instruction(instruction).unwrap(),
            ..Default::default()
        },
        &stablecoin_with_token_deps(),
    )
    .unwrap();

    let message = Message::from_circuit_output(vec![], output);
    let witness_set = WitnessSet::for_message(&message, proof, &[]);
    state
        .transition_from_privacy_preserving_transaction(
            &PrivacyPreservingTransaction::new(message, witness_set),
            0,
            0,
        )
        .unwrap();

    let position = Position::try_from(
        state
            .get_account_by_id(position_id)
            .data
            .shard(Ids::stablecoin_program()),
    )
    .expect("valid Position");
    assert_eq!(
        position.collateral_amount,
        position_collateral - withdraw_amount
    );

    match TokenHolding::try_from(
        state
            .get_account_by_id(Ids::user_holding())
            .data
            .shard(Ids::token_program()),
    )
    .expect("valid holding")
    {
        TokenHolding::Fungible { balance, .. } => {
            assert_eq!(balance, Balances::user_holding_init() + withdraw_amount);
        }
        TokenHolding::NftMaster { .. } | TokenHolding::NftPrintedCopy { .. } => {
            panic!("expected Fungible destination holding")
        }
    }

    let owner_expected = Account {
        nonce: Nonce::private_account_nonce_init(&owner_id),
        ..Account::default()
    };
    assert!(state
        .get_proof_for_commitment(&Commitment::new(&owner_id, &owner_expected))
        .is_some());
}

/// Protocol parameters the initialized-protocol helper installs. Kept as
/// constants so the poke tests can reason about the interval / staleness gates.
/// Wall clock for the open/withdraw fixture, in Unix milliseconds.
const OPEN_POSITION_NOW: u64 = 1_700_000_000_000;

/// Amounts for the full-lifecycle test.
const LIFECYCLE_OPEN_COLLATERAL: u128 = 500_000;
const LIFECYCLE_TOP_UP: u128 = 100_000;
const LIFECYCLE_BORROW: u128 = 100_000;
/// Stablecoin the owner holds before borrowing, standing in for a market purchase.
/// Needed because accrued fees make full repayment cost more than was minted.
const LIFECYCLE_STABLECOIN_BUFFER: u128 = 50_000;

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
        // Under shards a native balance is what keeps this otherwise unchanged
        // account non-empty and present in every diff.
        Account::funded(1),
    );

    let instruction = stablecoin_core::Instruction::InitializeProgram {
        now,
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
            ours(Ids::admin()),
            ours(compute_protocol_parameters_pda(Ids::stablecoin_program())),
            ours(compute_stability_fee_accumulator_pda(
                Ids::stablecoin_program(),
            )),
            ours(compute_redemption_price_state_pda(Ids::stablecoin_program())),
            token_shard(Ids::stablecoin_definition_pda()),
            token_shard(compute_stablecoin_master_holding_pda(
                Ids::stablecoin_program(),
            )),
            token_shard(Ids::collateral_definition()),
            ProgramShardSelector::new(Ids::oracle(), Ids::oracle_program()),
            clock_shard(),
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
    accounts: Vec<ProgramShardSelector>,
) -> Result<(), LeeError> {
    seed_clock(state, now);
    let mut slots = vec![ours(Ids::admin())];
    slots.extend(accounts);
    let message = public_transaction::Message::try_new(
        Ids::stablecoin_program(),
        slots,
        vec![current_nonce(state, Ids::admin())],
        instruction,
    )
    .expect("valid poke message");
    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::admin()]);
    let tx = PublicTransaction::new(message, witness_set);
    // v0.2.5 returns the transaction's emitted events; these helpers report success only.
    state
        .transition_from_public_transaction(&tx, block_id, now)
        .map(|_events| ())
}

/// A really-bootstrapped protocol plus everything a position owner needs: a
/// non-default-owned owner and freeze authority, a funded collateral holding, a
/// fresh oracle observation, and a market-acquired stablecoin buffer (credited
/// to `total_supply` so the books stay consistent).
fn bootstrapped_owner_state(start: u64) -> V03State {
    let mut state = initialize_protocol(start, 0);
    state.force_insert_account(Ids::freeze_authority(), Account::funded(1));

    // The owner, their collateral, and a market-acquired stablecoin balance. The
    // buffer matters: stability fees mean repaying the debt costs more stablecoin
    // than was minted, which is exactly the spec's §2 "acquire from market" model.
    state.force_insert_account(Ids::owner(), Account::funded(1));
    state.force_insert_account(Ids::user_holding(), Accounts::user_holding_init());
    // A fresh observation: the bootstrap fixture's oracle is anchored at 0, which
    // `generate_debt`'s staleness gate rejects against a real wall clock.
    state.force_insert_account(
        Ids::oracle(),
        Accounts::oracle_with(
            Ids::stablecoin_definition_pda(),
            Ids::collateral_definition(),
            stablecoin_core::math::FIXED_POINT_ONE / 4,
            start,
        ),
    );
    state.force_insert_account(
        Ids::user_stablecoin_holding(),
        Account::default().with_shard(
            Ids::token_program(),
            ShardData::from(&TokenHolding::Fungible {
                definition_id: Ids::stablecoin_definition_pda(),
                balance: LIFECYCLE_STABLECOIN_BUFFER,
            }),
        ),
    );
    // Credit the buffer to `total_supply` too. Force-inserting a holding without
    // it would leave the token's books inconsistent, and the chained Burn during
    // repayment would underflow.
    let mut definition = state.get_account_by_id(Ids::stablecoin_definition_pda());
    let bootstrapped = TokenDefinition::try_from(definition.data.shard(Ids::token_program()))
        .expect("valid TokenDefinition");
    match bootstrapped {
        TokenDefinition::Fungible {
            name,
            total_supply,
            metadata_id,
            authority,
        } => {
            definition.data.set_shard(
                Ids::token_program(),
                ShardData::from(&TokenDefinition::Fungible {
                    name,
                    total_supply: total_supply
                        .checked_add(LIFECYCLE_STABLECOIN_BUFFER)
                        .expect("seeded supply fits"),
                    metadata_id,
                    authority,
                }),
            );
        }
        TokenDefinition::NonFungible { .. } => panic!("stablecoin definition must be fungible"),
    }
    state.force_insert_account(Ids::stablecoin_definition_pda(), definition);

    state
}

/// Full position lifecycle through the zkVM against a really-bootstrapped
/// protocol: open → deposit → borrow → accrue → repay → withdraw → close.
///
/// This is "Mode A" — Plan 2 has shipped, so `accrue_stability_fee` runs between
/// borrowing and repaying and the §6.3 rounding is exercised at a non-trivial
/// accumulator rather than at exactly 1.0.
#[test]
fn stablecoin_full_position_lifecycle() {
    use stablecoin_core::math::{compute_current_accumulated_rate, mul_div_ceil, FIXED_POINT_ONE};

    let start: u64 = 1_700_000_000_000;
    let mut state = bootstrapped_owner_state(start);

    let mut block = 1u64;

    let params_pda = compute_protocol_parameters_pda(Ids::stablecoin_program());
    let accumulator_pda = compute_stability_fee_accumulator_pda(Ids::stablecoin_program());
    let redemption_pda = compute_redemption_price_state_pda(Ids::stablecoin_program());

    // 1. Open with collateral.
    let decl_parameters = declared_parameters(&state);
    let decl_now = start;
    submit(
        &mut state,
        &mut block,
        start,
        vec![
            ours(Ids::owner()),
            ours(Ids::position()),
            token_shard(Ids::vault()),
            token_shard(Ids::user_holding()),
            token_shard(Ids::collateral_definition()),
            ours(params_pda),
            clock_shard(),
        ],
        vec![
            (&Keys::owner(), Ids::owner()),
            (&Keys::user_holding(), Ids::user_holding()),
        ],
        stablecoin_core::Instruction::OpenPosition {
            parameters: decl_parameters,
            now: decl_now,
            position_nonce: Ids::position_nonce(),
            initial_collateral_amount: LIFECYCLE_OPEN_COLLATERAL,
        },
        "open_position",
    );
    assert_position(&state, LIFECYCLE_OPEN_COLLATERAL);

    // 2. Top up.
    let decl_parameters = declared_parameters(&state);
    let decl_vault_balance = declared_vault_balance(&state, Ids::vault());
    let decl_identity = declared_identity(&state, Ids::position());
    submit(
        &mut state,
        &mut block,
        start,
        vec![
            ours(Ids::owner()),
            ours(Ids::position()),
            token_shard(Ids::vault()),
            token_shard(Ids::user_holding()),
            ours(params_pda),
        ],
        vec![
            (&Keys::owner(), Ids::owner()),
            (&Keys::user_holding(), Ids::user_holding()),
        ],
        stablecoin_core::Instruction::DepositCollateral {
            expected: decl_identity,
            vault_balance: decl_vault_balance,
            parameters: decl_parameters,
            amount: LIFECYCLE_TOP_UP,
        },
        "deposit_collateral",
    );
    let collateral = LIFECYCLE_OPEN_COLLATERAL + LIFECYCLE_TOP_UP;
    assert_position(&state, collateral);
    assert_fungible_balance(&state, Ids::vault(), collateral);

    // 3. Borrow. The accumulator is still exactly 1.0, so normalized == nominal.
    let declared_oracle_reading = declared_oracle(&state);
    let decl_accumulator = declared_accumulator(&state);
    let decl_parameters = declared_parameters(&state);
    let decl_now = start;
    let decl_redemption = declared_redemption(&state);
    let decl_identity = declared_identity(&state, Ids::position());
    submit(
        &mut state,
        &mut block,
        start,
        vec![
            ours(Ids::owner()),
            ours(Ids::position()),
            token_shard(Ids::stablecoin_definition_pda()),
            token_shard(Ids::user_stablecoin_holding()),
            ours(accumulator_pda),
            ours(redemption_pda),
            ProgramShardSelector::new(Ids::oracle(), Ids::oracle_program()),
            ours(params_pda),
            clock_shard(),
        ],
        vec![(&Keys::owner(), Ids::owner())],
        stablecoin_core::Instruction::GenerateDebt {
            expected: decl_identity,
            parameters: decl_parameters,
            accumulator: decl_accumulator,
            redemption: decl_redemption,
            oracle_price: declared_oracle_reading.0,
            oracle_timestamp: declared_oracle_reading.1,
            now: decl_now,
            amount: LIFECYCLE_BORROW,
        },
        "generate_debt",
    );
    let borrowed = read_position(&state);
    assert_eq!(borrowed.normalized_debt_amount, LIFECYCLE_BORROW);
    assert_fungible_balance_of(
        &state,
        Ids::user_stablecoin_holding(),
        Ids::stablecoin_definition_pda(),
        LIFECYCLE_STABLECOIN_BUFFER + LIFECYCLE_BORROW,
    );
    assert_eq!(
        read_total_supply(&state, Ids::stablecoin_definition_pda()),
        LIFECYCLE_STABLECOIN_BUFFER + LIFECYCLE_BORROW,
        "the chained Mint must credit total_supply by exactly the amount minted"
    );

    // 4. Let an hour of fees accrue, then poke.
    let after_accrual = start + 3_600_000;
    let decl_parameters = declared_parameters(&state);
    let decl_now = after_accrual;
    submit_poke(
        &mut state,
        after_accrual,
        block,
        stablecoin_core::Instruction::AccrueStabilityFee {
            parameters: decl_parameters,
            now: decl_now,
        },
        vec![ours(params_pda), ours(accumulator_pda), clock_shard()],
    )
    .expect("accrue_stability_fee must succeed");
    block += 1;
    let accumulator = read_accumulator(&state);
    assert!(
        accumulator.accumulated_rate_at_last_accrual > FIXED_POINT_ONE,
        "an hour of stability fee must move the accumulator above 1.0"
    );

    // 5. Repay the whole debt. It costs more than was minted — that gap is the fee.
    let current_accumulator = compute_current_accumulated_rate(
        accumulator.accumulated_rate_at_last_accrual,
        protocol_config::STABILITY_FEE_PER_MILLISECOND,
        accumulator.last_accrued_at,
        after_accrual,
    );
    let nominal_debt = mul_div_ceil(
        borrowed.normalized_debt_amount,
        current_accumulator,
        FIXED_POINT_ONE,
    );
    assert!(
        nominal_debt > LIFECYCLE_BORROW,
        "accrued nominal debt must exceed the amount minted"
    );
    let decl_accumulator = declared_accumulator(&state);
    let decl_parameters = declared_parameters(&state);
    let decl_now = after_accrual;
    let decl_identity = declared_identity(&state, Ids::position());
    submit(
        &mut state,
        &mut block,
        after_accrual,
        vec![
            ours(Ids::owner()),
            ours(Ids::position()),
            token_shard(Ids::stablecoin_definition_pda()),
            token_shard(Ids::user_stablecoin_holding()),
            ours(accumulator_pda),
            ours(params_pda),
            clock_shard(),
        ],
        vec![
            (&Keys::owner(), Ids::owner()),
            (
                &Keys::user_stablecoin_holding(),
                Ids::user_stablecoin_holding(),
            ),
        ],
        stablecoin_core::Instruction::RepayDebt {
            expected: decl_identity,
            parameters: decl_parameters,
            accumulator: decl_accumulator,
            now: decl_now,
            amount: nominal_debt,
        },
        "repay_debt",
    );
    assert_eq!(read_position(&state).normalized_debt_amount, 0);
    // The books stay consistent, and the gap is the fee: the owner burned more
    // than was minted, so the buffer absorbed the difference.
    let remaining = LIFECYCLE_STABLECOIN_BUFFER + LIFECYCLE_BORROW - nominal_debt;
    assert_fungible_balance_of(
        &state,
        Ids::user_stablecoin_holding(),
        Ids::stablecoin_definition_pda(),
        remaining,
    );
    assert_eq!(
        read_total_supply(&state, Ids::stablecoin_definition_pda()),
        remaining,
        "total_supply must track the burn"
    );
    assert!(
        remaining < LIFECYCLE_STABLECOIN_BUFFER,
        "repaying accrued debt must cost more stablecoin than was minted"
    );

    // 6. Withdraw every last unit of collateral — allowed now the debt is gone.
    let decl_accumulator = declared_accumulator(&state);
    let decl_parameters = declared_parameters(&state);
    let decl_now = after_accrual;
    let decl_redemption = declared_redemption(&state);
    let decl_identity = declared_identity(&state, Ids::position());
    submit(
        &mut state,
        &mut block,
        after_accrual,
        vec![
            ours(Ids::owner()),
            ours(Ids::position()),
            token_shard(Ids::vault()),
            token_shard(Ids::user_holding()),
            ours(accumulator_pda),
            ours(redemption_pda),
            ours(params_pda),
            clock_shard(),
        ],
        vec![(&Keys::owner(), Ids::owner())],
        stablecoin_core::Instruction::WithdrawCollateral {
            expected: decl_identity,
            parameters: decl_parameters,
            accumulator: decl_accumulator,
            redemption: decl_redemption,
            now: decl_now,
            amount: collateral,
        },
        "withdraw_collateral",
    );
    assert_position(&state, 0);
    assert_fungible_balance(&state, Ids::vault(), 0);

    // 7. Close. The position is released; the vault lingers empty (§12).
    let decl_parameters = declared_parameters(&state);
    let decl_identity = declared_identity(&state, Ids::position());
    submit(
        &mut state,
        &mut block,
        after_accrual,
        vec![
            ours(Ids::owner()),
            ours(Ids::position()),
            token_shard(Ids::vault()),
            ours(params_pda),
        ],
        vec![(&Keys::owner(), Ids::owner())],
        stablecoin_core::Instruction::ClosePosition {
            expected: decl_identity,
            parameters: decl_parameters,
        },
        "close_position",
    );
    let closed = state.get_account_by_id(Ids::position());
    assert!(
        closed.data.shard(Ids::stablecoin_program()).is_empty(),
        "close_position must clear the position's shard"
    );
    // The account itself lingers: clearing a shard does not delete the account,
    // and LEE forbids a program from changing a `nonce`, so the PDA is not
    // released for reuse.
    assert_eq!(closed.nonce, state.get_account_by_id(Ids::position()).nonce);
    assert_fungible_balance(&state, Ids::vault(), 0);
}

fn submit(
    state: &mut V03State,
    block: &mut u64,
    now: u64,
    accounts: Vec<ProgramShardSelector>,
    signers: Vec<(&PrivateKey, AccountId)>,
    instruction: stablecoin_core::Instruction,
    label: &str,
) {
    try_submit(state, block, now, accounts, signers, instruction)
        .unwrap_or_else(|e| panic!("{label} must succeed: {e:?}"));
}

/// Like [`submit`], but hands the outcome back so a test can assert that an
/// instruction is *rejected* — and why. `block` advances only on success.
fn try_submit(
    state: &mut V03State,
    block: &mut u64,
    now: u64,
    accounts: Vec<ProgramShardSelector>,
    signers: Vec<(&PrivateKey, AccountId)>,
    instruction: stablecoin_core::Instruction,
) -> Result<(), LeeError> {
    seed_clock(state, now);
    let nonces = signers
        .iter()
        .map(|(_, id)| current_nonce(state, *id))
        .collect();
    let keys: Vec<&PrivateKey> = signers.iter().map(|(key, _)| *key).collect();
    let message = public_transaction::Message::try_new(
        Ids::stablecoin_program(),
        accounts,
        nonces,
        instruction,
    )
    .expect("valid message");
    let witness_set = public_transaction::WitnessSet::for_message(&message, &keys);
    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, *block, now)?;
    *block = block.saturating_add(1);
    Ok(())
}

/// Asserts a rejected transition failed inside the guest with `message`.
fn assert_rejected_with(result: Result<(), LeeError>, message: &str, label: &str) {
    let err = result.expect_err(&format!("{label} must be rejected"));
    let text = format!("{err:?}");
    assert!(
        text.contains(message),
        "{label}: expected rejection containing {message:?}, got {text}"
    );
}

fn read_total_supply(state: &V03State, definition_id: AccountId) -> u128 {
    let definition = TokenDefinition::try_from(
        state
            .get_account_by_id(definition_id)
            .data
            .shard(Ids::token_program()),
    )
    .expect("valid TokenDefinition");
    match definition {
        TokenDefinition::Fungible { total_supply, .. } => total_supply,
        TokenDefinition::NonFungible { .. } => panic!("expected a fungible definition"),
    }
}

/// Spec §10.10–10.16: the admin retunes parameters, then rotates the role away.
///
/// The rotation is the part worth proving end to end: it is one-step and
/// effective immediately, so the previous admin must be locked out on the very
/// next instruction.
#[test]
fn stablecoin_admin_retunes_parameters_then_rotates_the_role() {
    use stablecoin_core::math::FIXED_POINT_ONE;

    let start: u64 = 1_700_000_000_000;
    let mut state = initialize_protocol(start, 0);
    state.force_insert_account(Ids::freeze_authority(), Account::funded(1));
    let mut block = 1u64;
    let params_pda = compute_protocol_parameters_pda(Ids::stablecoin_program());
    let accumulator_pda = compute_stability_fee_accumulator_pda(Ids::stablecoin_program());
    let admin = (&Keys::admin(), Ids::admin());

    // A fee change auto-accrues the elapsed gap at the OLD rate first.
    let one_hour_later = start + 3_600_000;
    let new_rate = FIXED_POINT_ONE + 2_000_000_000_000_000;
    let decl_parameters = declared_parameters(&state);
    let decl_now = one_hour_later;
    submit(
        &mut state,
        &mut block,
        one_hour_later,
        vec![
            ours(Ids::admin()),
            ours(params_pda),
            ours(accumulator_pda),
            clock_shard(),
        ],
        vec![admin],
        stablecoin_core::Instruction::SetStabilityFeePerMillisecond {
            new_rate,
            old_rate: decl_parameters.stability_fee_per_millisecond,
            now: decl_now,
        },
        "set_stability_fee_per_millisecond",
    );
    assert_eq!(
        read_parameters(&state).stability_fee_per_millisecond,
        new_rate
    );
    let accrued = read_accumulator(&state);
    assert!(
        accrued.accumulated_rate_at_last_accrual > FIXED_POINT_ONE,
        "the elapsed hour must have accrued before the rate changed"
    );
    assert_eq!(accrued.last_accrued_at, one_hour_later);

    // The bundled setters.
    let new_ratio = FIXED_POINT_ONE * 2;
    submit(
        &mut state,
        &mut block,
        one_hour_later,
        vec![ours(Ids::admin()), ours(params_pda)],
        vec![admin],
        stablecoin_core::Instruction::SetMinimumCollateralizationRatio { new_ratio },
        "set_minimum_collateralization_ratio",
    );
    submit(
        &mut state,
        &mut block,
        one_hour_later,
        vec![ours(Ids::admin()), ours(params_pda)],
        vec![admin],
        stablecoin_core::Instruction::SetControllerGains {
            new_proportional_gain: -7,
            new_integral_gain: 11,
        },
        "set_controller_gains",
    );
    submit(
        &mut state,
        &mut block,
        one_hour_later,
        vec![ours(Ids::admin()), ours(params_pda)],
        vec![admin],
        stablecoin_core::Instruction::SetTimingParameters {
            new_minimum_milliseconds_between_rate_updates: 1_000,
            new_maximum_oracle_price_age_milliseconds: 2_000,
        },
        "set_timing_parameters",
    );
    let after_tuning = read_parameters(&state);
    assert_eq!(after_tuning.minimum_collateralization_ratio, new_ratio);
    assert_eq!(after_tuning.controller_proportional_gain, -7);
    assert_eq!(after_tuning.controller_integral_gain, 11);
    assert_eq!(
        after_tuning.minimum_milliseconds_between_rate_updates,
        1_000
    );
    assert_eq!(after_tuning.maximum_oracle_price_age_milliseconds, 2_000);

    // Out-of-band values are refused by the same §8 bands bootstrap enforces.
    assert_rejected_with(
        try_submit(
            &mut state,
            &mut block,
            one_hour_later,
            vec![ours(Ids::admin()), ours(params_pda)],
            vec![admin],
            stablecoin_core::Instruction::SetMinimumCollateralizationRatio {
                new_ratio: FIXED_POINT_ONE,
            },
        ),
        "new_minimum_collateralization_ratio below 1.1x",
        "set_minimum_collateralization_ratio below the band",
    );

    // The freeze authority is the admin's to set, not its own.
    assert_rejected_with(
        try_submit(
            &mut state,
            &mut block,
            one_hour_later,
            vec![ours(Ids::freeze_authority()), ours(params_pda)],
            vec![(&Keys::freeze_authority(), Ids::freeze_authority())],
            stablecoin_core::Instruction::SetFreezeAuthority {
                new_freeze_authority_account_id: Ids::freeze_authority(),
            },
        ),
        "Signer is not the protocol's admin",
        "set_freeze_authority by the freeze authority",
    );

    // Rotate the admin to the owner's handle, then confirm the old admin is out.
    submit(
        &mut state,
        &mut block,
        one_hour_later,
        vec![ours(Ids::admin()), ours(params_pda)],
        vec![admin],
        stablecoin_core::Instruction::SetAdmin {
            new_admin_account_id: Ids::owner(),
        },
        "set_admin",
    );
    assert_eq!(read_parameters(&state).admin_account_id, Ids::owner());

    assert_rejected_with(
        try_submit(
            &mut state,
            &mut block,
            one_hour_later,
            vec![ours(Ids::admin()), ours(params_pda)],
            vec![admin],
            stablecoin_core::Instruction::SetControllerGains {
                new_proportional_gain: 0,
                new_integral_gain: 0,
            },
        ),
        "Signer is not the protocol's admin",
        "the rotated-out admin",
    );

    // And the new admin can act immediately.
    state.force_insert_account(Ids::owner(), Account::funded(1));
    submit(
        &mut state,
        &mut block,
        one_hour_later,
        vec![ours(Ids::owner()), ours(params_pda)],
        vec![(&Keys::owner(), Ids::owner())],
        stablecoin_core::Instruction::SetControllerGains {
            new_proportional_gain: 3,
            new_integral_gain: 4,
        },
        "set_controller_gains by the new admin",
    );
    assert_eq!(read_parameters(&state).controller_proportional_gain, 3);
}

/// Spec §16.2: the freeze authority halts the risk-increasing instructions.
/// Everything that reduces risk — plus the permissionless pokes — keeps working.
#[test]
fn stablecoin_freeze_blocks_risky_ops() {
    let start: u64 = 1_700_000_000_000;
    let mut state = bootstrapped_owner_state(start);
    let mut block = 1u64;
    let params_pda = compute_protocol_parameters_pda(Ids::stablecoin_program());
    let accumulator_pda = compute_stability_fee_accumulator_pda(Ids::stablecoin_program());
    let redemption_pda = compute_redemption_price_state_pda(Ids::stablecoin_program());
    let owner = (&Keys::owner(), Ids::owner());
    let user_holding = (&Keys::user_holding(), Ids::user_holding());
    let freeze_authority = (&Keys::freeze_authority(), Ids::freeze_authority());

    // Normal operation: open, then borrow so the frozen-path assertions below
    // exercise a position that actually carries debt.
    let decl_parameters = declared_parameters(&state);
    let decl_now = start;
    submit(
        &mut state,
        &mut block,
        start,
        vec![
            ours(Ids::owner()),
            ours(Ids::position()),
            token_shard(Ids::vault()),
            token_shard(Ids::user_holding()),
            token_shard(Ids::collateral_definition()),
            ours(params_pda),
            clock_shard(),
        ],
        vec![owner, user_holding],
        stablecoin_core::Instruction::OpenPosition {
            parameters: decl_parameters,
            now: decl_now,
            position_nonce: Ids::position_nonce(),
            initial_collateral_amount: LIFECYCLE_OPEN_COLLATERAL,
        },
        "open_position",
    );
    let generate_debt_accounts = vec![
        ours(Ids::owner()),
        ours(Ids::position()),
        token_shard(Ids::stablecoin_definition_pda()),
        token_shard(Ids::user_stablecoin_holding()),
        ours(accumulator_pda),
        ours(redemption_pda),
        ProgramShardSelector::new(Ids::oracle(), Ids::oracle_program()),
        ours(params_pda),
        clock_shard(),
    ];
    let declared_oracle_reading = declared_oracle(&state);
    let decl_accumulator = declared_accumulator(&state);
    let decl_parameters = declared_parameters(&state);
    let decl_now = start;
    let decl_redemption = declared_redemption(&state);
    let decl_identity = declared_identity(&state, Ids::position());
    submit(
        &mut state,
        &mut block,
        start,
        generate_debt_accounts.clone(),
        vec![owner],
        stablecoin_core::Instruction::GenerateDebt {
            expected: decl_identity,
            parameters: decl_parameters,
            accumulator: decl_accumulator,
            redemption: decl_redemption,
            oracle_price: declared_oracle_reading.0,
            oracle_timestamp: declared_oracle_reading.1,
            now: decl_now,
            amount: LIFECYCLE_BORROW,
        },
        "generate_debt before the freeze",
    );

    // Only the bound freeze authority may freeze — the admin is not it.
    assert_rejected_with(
        try_submit(
            &mut state,
            &mut block,
            start,
            vec![ours(Ids::admin()), ours(params_pda)],
            vec![(&Keys::admin(), Ids::admin())],
            stablecoin_core::Instruction::Freeze,
        ),
        "Signer is not the protocol's freeze authority",
        "freeze by the admin",
    );

    submit(
        &mut state,
        &mut block,
        start,
        vec![ours(Ids::freeze_authority()), ours(params_pda)],
        vec![freeze_authority],
        stablecoin_core::Instruction::Freeze,
        "freeze",
    );
    assert!(read_parameters(&state).is_frozen);

    // Blocked: the three risk-increasing instructions.
    let declared_oracle_reading = declared_oracle(&state);
    let decl_accumulator = declared_accumulator(&state);
    let decl_parameters = declared_parameters(&state);
    let decl_now = start;
    let decl_redemption = declared_redemption(&state);
    let decl_identity = declared_identity(&state, Ids::position());
    assert_rejected_with(
        try_submit(
            &mut state,
            &mut block,
            start,
            generate_debt_accounts.clone(),
            vec![owner],
            stablecoin_core::Instruction::GenerateDebt {
                expected: decl_identity,
                parameters: decl_parameters,
                accumulator: decl_accumulator,
                redemption: decl_redemption,
                oracle_price: declared_oracle_reading.0,
                oracle_timestamp: declared_oracle_reading.1,
                now: decl_now,
                amount: 1,
            },
        ),
        "Protocol is frozen",
        "generate_debt while frozen",
    );
    let decl_accumulator = declared_accumulator(&state);
    let decl_parameters = declared_parameters(&state);
    let decl_now = start;
    let decl_redemption = declared_redemption(&state);
    let decl_identity = declared_identity(&state, Ids::position());
    assert_rejected_with(
        try_submit(
            &mut state,
            &mut block,
            start,
            vec![
                ours(Ids::owner()),
                ours(Ids::position()),
                token_shard(Ids::vault()),
                token_shard(Ids::user_holding()),
                ours(accumulator_pda),
                ours(redemption_pda),
                ours(params_pda),
                clock_shard(),
            ],
            vec![owner],
            stablecoin_core::Instruction::WithdrawCollateral {
                expected: decl_identity,
                parameters: decl_parameters,
                accumulator: decl_accumulator,
                redemption: decl_redemption,
                now: decl_now,
                amount: 1,
            },
        ),
        "Protocol is frozen",
        "withdraw_collateral while frozen",
    );
    let second_nonce = Ids::position_nonce() + 1;
    let second_position =
        compute_position_pda(Ids::stablecoin_program(), Ids::owner(), second_nonce);
    let decl_parameters = declared_parameters(&state);
    let decl_now = start;
    assert_rejected_with(
        try_submit(
            &mut state,
            &mut block,
            start,
            vec![
                ours(Ids::owner()),
                ours(second_position),
                token_shard(compute_position_vault_pda(
                    Ids::stablecoin_program(),
                    second_position,
                )),
                token_shard(Ids::user_holding()),
                token_shard(Ids::collateral_definition()),
                ours(params_pda),
                clock_shard(),
            ],
            vec![owner, user_holding],
            stablecoin_core::Instruction::OpenPosition {
                parameters: decl_parameters,
                now: decl_now,
                position_nonce: second_nonce,
                initial_collateral_amount: 1,
            },
        ),
        "Protocol is frozen",
        "open_position while frozen",
    );

    // Still allowed: everything that reduces risk, and the pokes.
    let decl_parameters = declared_parameters(&state);
    let decl_vault_balance = declared_vault_balance(&state, Ids::vault());
    let decl_identity = declared_identity(&state, Ids::position());
    submit(
        &mut state,
        &mut block,
        start,
        vec![
            ours(Ids::owner()),
            ours(Ids::position()),
            token_shard(Ids::vault()),
            token_shard(Ids::user_holding()),
            ours(params_pda),
        ],
        vec![owner, user_holding],
        stablecoin_core::Instruction::DepositCollateral {
            expected: decl_identity,
            vault_balance: decl_vault_balance,
            parameters: decl_parameters,
            amount: LIFECYCLE_TOP_UP,
        },
        "deposit_collateral while frozen",
    );
    let decl_accumulator = declared_accumulator(&state);
    let decl_parameters = declared_parameters(&state);
    let decl_now = start;
    let decl_identity = declared_identity(&state, Ids::position());
    submit(
        &mut state,
        &mut block,
        start,
        vec![
            ours(Ids::owner()),
            ours(Ids::position()),
            token_shard(Ids::stablecoin_definition_pda()),
            token_shard(Ids::user_stablecoin_holding()),
            ours(accumulator_pda),
            ours(params_pda),
            clock_shard(),
        ],
        vec![
            owner,
            (
                &Keys::user_stablecoin_holding(),
                Ids::user_stablecoin_holding(),
            ),
        ],
        stablecoin_core::Instruction::RepayDebt {
            expected: decl_identity,
            parameters: decl_parameters,
            accumulator: decl_accumulator,
            now: decl_now,
            amount: LIFECYCLE_BORROW / 2,
        },
        "repay_debt while frozen",
    );
    let decl_parameters = declared_parameters(&state);
    let decl_now = start + 60_000;
    submit_poke(
        &mut state,
        start + 60_000,
        block,
        stablecoin_core::Instruction::AccrueStabilityFee {
            parameters: decl_parameters,
            now: decl_now,
        },
        vec![ours(params_pda), ours(accumulator_pda), clock_shard()],
    )
    .expect("accrue_stability_fee must work while frozen");
    block += 1;

    // Freezing twice is a successful no-op.
    submit(
        &mut state,
        &mut block,
        start + 60_000,
        vec![ours(Ids::freeze_authority()), ours(params_pda)],
        vec![freeze_authority],
        stablecoin_core::Instruction::Freeze,
        "second freeze",
    );
    assert!(read_parameters(&state).is_frozen);
    let position = read_position(&state);
    assert_eq!(
        position.collateral_amount,
        LIFECYCLE_OPEN_COLLATERAL + LIFECYCLE_TOP_UP
    );
    assert_eq!(position.normalized_debt_amount, LIFECYCLE_BORROW / 2);
}

/// Spec §10.18: `unfreeze` restores the blocked instructions, and is idempotent.
#[test]
fn stablecoin_unfreeze_resumes_normal_operation() {
    let start: u64 = 1_700_000_000_000;
    let mut state = bootstrapped_owner_state(start);
    let mut block = 1u64;
    let params_pda = compute_protocol_parameters_pda(Ids::stablecoin_program());
    let owner = (&Keys::owner(), Ids::owner());
    let freeze_authority = (&Keys::freeze_authority(), Ids::freeze_authority());

    let decl_parameters = declared_parameters(&state);
    let decl_now = start;
    submit(
        &mut state,
        &mut block,
        start,
        vec![
            ours(Ids::owner()),
            ours(Ids::position()),
            token_shard(Ids::vault()),
            token_shard(Ids::user_holding()),
            token_shard(Ids::collateral_definition()),
            ours(params_pda),
            clock_shard(),
        ],
        vec![owner, (&Keys::user_holding(), Ids::user_holding())],
        stablecoin_core::Instruction::OpenPosition {
            parameters: decl_parameters,
            now: decl_now,
            position_nonce: Ids::position_nonce(),
            initial_collateral_amount: LIFECYCLE_OPEN_COLLATERAL,
        },
        "open_position",
    );
    let withdraw_accounts = vec![
        ours(Ids::owner()),
        ours(Ids::position()),
        token_shard(Ids::vault()),
        token_shard(Ids::user_holding()),
        ours(compute_stability_fee_accumulator_pda(
            Ids::stablecoin_program(),
        )),
        ours(compute_redemption_price_state_pda(Ids::stablecoin_program())),
        ours(params_pda),
        clock_shard(),
    ];

    submit(
        &mut state,
        &mut block,
        start,
        vec![ours(Ids::freeze_authority()), ours(params_pda)],
        vec![freeze_authority],
        stablecoin_core::Instruction::Freeze,
        "freeze",
    );
    let decl_accumulator = declared_accumulator(&state);
    let decl_parameters = declared_parameters(&state);
    let decl_now = start;
    let decl_redemption = declared_redemption(&state);
    let decl_identity = declared_identity(&state, Ids::position());
    assert_rejected_with(
        try_submit(
            &mut state,
            &mut block,
            start,
            withdraw_accounts.clone(),
            vec![owner],
            stablecoin_core::Instruction::WithdrawCollateral {
                expected: decl_identity,
                parameters: decl_parameters,
                accumulator: decl_accumulator,
                redemption: decl_redemption,
                now: decl_now,
                amount: 1,
            },
        ),
        "Protocol is frozen",
        "withdraw_collateral while frozen",
    );

    // The admin cannot lift a freeze either.
    assert_rejected_with(
        try_submit(
            &mut state,
            &mut block,
            start,
            vec![ours(Ids::admin()), ours(params_pda)],
            vec![(&Keys::admin(), Ids::admin())],
            stablecoin_core::Instruction::Unfreeze,
        ),
        "Signer is not the protocol's freeze authority",
        "unfreeze by the admin",
    );

    submit(
        &mut state,
        &mut block,
        start,
        vec![ours(Ids::freeze_authority()), ours(params_pda)],
        vec![freeze_authority],
        stablecoin_core::Instruction::Unfreeze,
        "unfreeze",
    );
    assert!(!read_parameters(&state).is_frozen);

    let decl_accumulator = declared_accumulator(&state);
    let decl_parameters = declared_parameters(&state);
    let decl_now = start;
    let decl_redemption = declared_redemption(&state);
    let decl_identity = declared_identity(&state, Ids::position());
    submit(
        &mut state,
        &mut block,
        start,
        withdraw_accounts,
        vec![owner],
        stablecoin_core::Instruction::WithdrawCollateral {
            expected: decl_identity,
            parameters: decl_parameters,
            accumulator: decl_accumulator,
            redemption: decl_redemption,
            now: decl_now,
            amount: 1,
        },
        "withdraw_collateral after the unfreeze",
    );
    assert_eq!(
        read_position(&state).collateral_amount,
        LIFECYCLE_OPEN_COLLATERAL - 1
    );

    // Unfreezing twice is a successful no-op.
    submit(
        &mut state,
        &mut block,
        start,
        vec![ours(Ids::freeze_authority()), ours(params_pda)],
        vec![freeze_authority],
        stablecoin_core::Instruction::Unfreeze,
        "second unfreeze",
    );
    assert!(!read_parameters(&state).is_frozen);
}

fn read_parameters(state: &V03State) -> stablecoin_core::ProtocolParameters {
    stablecoin_core::ProtocolParameters::try_from(
        state
            .get_account_by_id(compute_protocol_parameters_pda(Ids::stablecoin_program()))
            .data
            .shard(Ids::stablecoin_program()),
    )
    .expect("valid ProtocolParameters")
}

fn read_position(state: &V03State) -> Position {
    Position::try_from(
        state
            .get_account_by_id(Ids::position())
            .data
            .shard(Ids::stablecoin_program()),
    )
    .expect("valid Position")
}

fn read_accumulator(state: &V03State) -> stablecoin_core::StabilityFeeAccumulator {
    stablecoin_core::StabilityFeeAccumulator::try_from(
        state
            .get_account_by_id(compute_stability_fee_accumulator_pda(
                Ids::stablecoin_program(),
            ))
            .data
            .shard(Ids::stablecoin_program()),
    )
    .expect("valid StabilityFeeAccumulator")
}

fn read_redemption_price_state(state: &V03State) -> stablecoin_core::RedemptionPriceState {
    stablecoin_core::RedemptionPriceState::try_from(
        state
            .get_account_by_id(compute_redemption_price_state_pda(Ids::stablecoin_program()))
            .data
            .shard(Ids::stablecoin_program()),
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
        state
            .get_account_by_id(compute_protocol_parameters_pda(Ids::stablecoin_program()))
            .data
            .shard(Ids::stablecoin_program()),
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
        state
            .get_account_by_id(compute_stability_fee_accumulator_pda(
                Ids::stablecoin_program(),
            ))
            .data
            .shard(Ids::stablecoin_program()),
    )
    .expect("valid StabilityFeeAccumulator");
    assert_eq!(acc.accumulated_rate_at_last_accrual, FIXED_POINT_ONE);
    assert_eq!(acc.last_accrued_at, now);

    // Redemption price anchored at the initial value / now.
    let rp = stablecoin_core::RedemptionPriceState::try_from(
        state
            .get_account_by_id(compute_redemption_price_state_pda(Ids::stablecoin_program()))
            .data
            .shard(Ids::stablecoin_program()),
    )
    .expect("valid RedemptionPriceState");
    assert_eq!(rp.redemption_price_at_last_update, FIXED_POINT_ONE / 2);
    assert_eq!(rp.redemption_rate_per_millisecond, FIXED_POINT_ONE);
    assert_eq!(rp.controller_integral_term, 0);
    assert_eq!(rp.last_updated_at, now);

    // Stablecoin definition created via the chained Token::NewFungibleDefinition.
    let definition = TokenDefinition::try_from(
        state
            .get_account_by_id(Ids::stablecoin_definition_pda())
            .data
            .shard(Ids::token_program()),
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
        state
            .get_account_by_id(compute_stablecoin_master_holding_pda(
                Ids::stablecoin_program(),
            ))
            .data
            .shard(Ids::token_program()),
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
    let decl_parameters = declared_parameters(&state);
    let decl_now = now;
    submit_poke(
        &mut state,
        now,
        2,
        stablecoin_core::Instruction::AccrueStabilityFee {
            parameters: decl_parameters,
            now: decl_now,
        },
        vec![
            ours(compute_protocol_parameters_pda(Ids::stablecoin_program())),
            ours(compute_stability_fee_accumulator_pda(
                Ids::stablecoin_program(),
            )),
            clock_shard(),
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

    let decl_oracle = declared_oracle(&state);
    let decl_parameters = declared_parameters(&state);
    let decl_now = now;
    submit_poke(
        &mut state,
        now,
        2,
        stablecoin_core::Instruction::UpdateRedemptionRate {
            parameters: decl_parameters,
            oracle_price: decl_oracle.0,
            oracle_timestamp: decl_oracle.1,
            now: decl_now,
        },
        vec![
            ours(compute_protocol_parameters_pda(Ids::stablecoin_program())),
            ours(compute_redemption_price_state_pda(Ids::stablecoin_program())),
            ProgramShardSelector::new(Ids::oracle(), Ids::oracle_program()),
            clock_shard(),
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
            ours(compute_protocol_parameters_pda(Ids::stablecoin_program())),
            ours(compute_stability_fee_accumulator_pda(
                Ids::stablecoin_program(),
            )),
            ours(compute_redemption_price_state_pda(Ids::stablecoin_program())),
            ProgramShardSelector::new(Ids::oracle(), Ids::oracle_program()),
            clock_shard(),
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

    let decl_oracle = declared_oracle(&state);
    let decl_parameters = declared_parameters(&state);
    let decl_now = first;
    submit_poke(
        &mut state,
        first,
        2,
        stablecoin_core::Instruction::RefreshGlobals {
            parameters: decl_parameters,
            oracle_price: decl_oracle.0,
            oracle_timestamp: decl_oracle.1,
            now: decl_now,
        },
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
    let decl_oracle = declared_oracle(&state);
    let decl_parameters = declared_parameters(&state);
    let decl_now = second;
    submit_poke(
        &mut state,
        second,
        3,
        stablecoin_core::Instruction::RefreshGlobals {
            parameters: decl_parameters,
            oracle_price: decl_oracle.0,
            oracle_timestamp: decl_oracle.1,
            now: decl_now,
        },
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
