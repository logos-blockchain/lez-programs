use amm_client::{
    discovery::{
        canonical_pair, derive_config_id, derive_pair_read_manifest, inspect_config, inspect_pair,
        MissingVaultState, PairInspection, PairReadSnapshots,
    },
    quote::AccountSnapshot,
};
use amm_core::{AmmConfig, PoolDefinition, FEE_TIER_BPS_30};
use amm_program::quote::PairOrder;
use clock_core::ClockAccountData;
use nssa_core::{
    account::{Account, AccountId, Data, Nonce},
    program::ProgramId,
};
use token_core::{TokenDefinition, TokenHolding};
use twap_oracle_core::CurrentTickAccount;

const AMM_PROGRAM_ID: ProgramId = [42; 8];
const TOKEN_PROGRAM_ID: ProgramId = [15; 8];
const TWAP_ORACLE_PROGRAM_ID: ProgramId = [77; 8];

fn lower_token_id() -> AccountId {
    AccountId::new([1; 32])
}

fn higher_token_id() -> AccountId {
    AccountId::new([2; 32])
}

fn account(program_owner: ProgramId, data: Data) -> Account {
    Account {
        program_owner,
        balance: 0,
        data,
        nonce: Nonce(0),
    }
}

fn config_snapshot() -> AccountSnapshot {
    AccountSnapshot::new(
        derive_config_id(AMM_PROGRAM_ID),
        account(
            AMM_PROGRAM_ID,
            Data::from(&AmmConfig {
                token_program_id: TOKEN_PROGRAM_ID,
                twap_oracle_program_id: TWAP_ORACLE_PROGRAM_ID,
                authority: AccountId::new([9; 32]),
            }),
        ),
    )
}

fn fungible_definition(
    id: AccountId,
    total_supply: u128,
    authority: Option<AccountId>,
) -> AccountSnapshot {
    AccountSnapshot::new(
        id,
        account(
            TOKEN_PROGRAM_ID,
            Data::from(&TokenDefinition::Fungible {
                name: String::from("Token"),
                total_supply,
                metadata_id: None,
                authority,
            }),
        ),
    )
}

fn fungible_holding(
    id: AccountId,
    program_owner: ProgramId,
    definition_id: AccountId,
    balance: u128,
) -> AccountSnapshot {
    AccountSnapshot::new(
        id,
        account(
            program_owner,
            Data::from(&TokenHolding::Fungible {
                definition_id,
                balance,
            }),
        ),
    )
}

fn clock_snapshot(id: AccountId) -> AccountSnapshot {
    let data = ClockAccountData {
        block_id: 123,
        timestamp: 456,
    }
    .to_bytes();
    AccountSnapshot::new(
        id,
        account([88; 8], Data::try_from(data).expect("clock data must fit")),
    )
}

#[test]
fn config_and_pair_discovery_are_canonical_and_caller_ordered() {
    let config = config_snapshot();
    let context = inspect_config(AMM_PROGRAM_ID, &config).expect("config must validate");
    let forward = derive_pair_read_manifest(&context, lower_token_id(), higher_token_id())
        .expect("distinct pair must derive");
    let reverse = derive_pair_read_manifest(&context, higher_token_id(), lower_token_id())
        .expect("distinct pair must derive");

    assert_eq!(derive_config_id(AMM_PROGRAM_ID), config.account_id());
    assert_eq!(context.token_program_id(), TOKEN_PROGRAM_ID);
    assert_eq!(context.twap_oracle_program_id(), TWAP_ORACLE_PROGRAM_ID);
    assert_eq!(
        canonical_pair(lower_token_id(), higher_token_id())
            .expect("distinct pair must canonicalize")
            .token_a_id(),
        higher_token_id()
    );
    assert_eq!(forward.pool_id(), reverse.pool_id());
    assert_eq!(forward.first_token().definition_id(), lower_token_id());
    assert_eq!(reverse.second_token().definition_id(), lower_token_id());
    assert_eq!(
        forward.vault_id_for(lower_token_id()),
        reverse.vault_id_for(lower_token_id())
    );
    assert_eq!(forward.config_id(), config.account_id());
    assert_eq!(forward.clock_id(), clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID);
}

#[test]
fn missing_pair_allows_transfer_compatible_existing_vault() {
    let context = inspect_config(AMM_PROGRAM_ID, &config_snapshot()).expect("config must validate");
    let manifest = derive_pair_read_manifest(&context, lower_token_id(), higher_token_id())
        .expect("pair must derive");
    let pool = AccountSnapshot::new(manifest.pool_id(), Account::default());
    let first_definition = fungible_definition(lower_token_id(), 10_000, None);
    let second_definition = fungible_definition(higher_token_id(), 20_000, None);
    let first_vault = fungible_holding(
        manifest.first_token().vault_id(),
        TOKEN_PROGRAM_ID,
        lower_token_id(),
        7,
    );
    let second_vault = AccountSnapshot::new(manifest.second_token().vault_id(), Account::default());
    let liquidity_definition =
        AccountSnapshot::new(manifest.liquidity_definition_id(), Account::default());
    let lp_lock = AccountSnapshot::new(manifest.lp_lock_holding_id(), Account::default());
    let current_tick = AccountSnapshot::new(manifest.current_tick_id(), Account::default());
    let clock = clock_snapshot(manifest.clock_id());

    let inspected = inspect_pair(
        &context,
        lower_token_id(),
        higher_token_id(),
        PairReadSnapshots {
            pool: &pool,
            first_token_definition: &first_definition,
            second_token_definition: &second_definition,
            first_token_vault: &first_vault,
            second_token_vault: &second_vault,
            liquidity_definition: &liquidity_definition,
            lp_lock_holding: &lp_lock,
            current_tick: &current_tick,
            clock: &clock,
        },
    )
    .expect("current pool-creation preconditions must validate");

    let PairInspection::Missing(missing) = inspected else {
        panic!("default pool must inspect as missing");
    };
    assert_eq!(
        missing.first_vault(),
        MissingVaultState::ExistingFungible { balance: 7 }
    );
    assert_eq!(missing.second_vault(), MissingVaultState::Uninitialized);
    assert_eq!(missing.clock().block_id(), 123);
    assert_eq!(missing.clock().timestamp(), 456);

    let foreign_vault = fungible_holding(
        manifest.first_token().vault_id(),
        [99; 8],
        lower_token_id(),
        7,
    );
    let error = inspect_pair(
        &context,
        lower_token_id(),
        higher_token_id(),
        PairReadSnapshots {
            pool: &pool,
            first_token_definition: &first_definition,
            second_token_definition: &second_definition,
            first_token_vault: &foreign_vault,
            second_token_vault: &second_vault,
            liquidity_definition: &liquidity_definition,
            lp_lock_holding: &lp_lock,
            current_tick: &current_tick,
            clock: &clock,
        },
    )
    .err()
    .expect("foreign-owned existing vault cannot be mutated by Token Program");
    assert!(matches!(
        error,
        amm_client::ClientError::ProgramOwnerMismatch {
            account: "first token vault",
            ..
        }
    ));
}

#[test]
fn active_pair_maps_caller_order_to_stored_pool_order() {
    let context = inspect_config(AMM_PROGRAM_ID, &config_snapshot()).expect("config must validate");
    let manifest = derive_pair_read_manifest(&context, lower_token_id(), higher_token_id())
        .expect("pair must derive");
    let lp_id = manifest.liquidity_definition_id();
    let pool_definition = PoolDefinition {
        // Stored pool order is opposite the caller's lower/higher order.
        definition_token_a_id: higher_token_id(),
        definition_token_b_id: lower_token_id(),
        vault_a_id: manifest.second_token().vault_id(),
        vault_b_id: manifest.first_token().vault_id(),
        liquidity_pool_id: lp_id,
        liquidity_pool_supply: 2_000,
        reserve_a: 1_000,
        reserve_b: 500,
        fees: FEE_TIER_BPS_30,
    };
    let pool = AccountSnapshot::new(
        manifest.pool_id(),
        account(AMM_PROGRAM_ID, Data::from(&pool_definition)),
    );
    let first_definition = fungible_definition(lower_token_id(), 10_000, None);
    let second_definition = fungible_definition(higher_token_id(), 20_000, None);
    let first_vault = fungible_holding(
        manifest.first_token().vault_id(),
        TOKEN_PROGRAM_ID,
        lower_token_id(),
        550,
    );
    let second_vault = fungible_holding(
        manifest.second_token().vault_id(),
        TOKEN_PROGRAM_ID,
        higher_token_id(),
        1_100,
    );
    let liquidity_definition = fungible_definition(lp_id, 2_000, Some(lp_id));
    let lp_lock = fungible_holding(
        manifest.lp_lock_holding_id(),
        TOKEN_PROGRAM_ID,
        lp_id,
        1_000,
    );
    let current_tick = AccountSnapshot::new(
        manifest.current_tick_id(),
        account(
            TWAP_ORACLE_PROGRAM_ID,
            Data::from(&CurrentTickAccount {
                tick: -1,
                last_updated: 400,
            }),
        ),
    );
    let clock = clock_snapshot(manifest.clock_id());

    let inspected = inspect_pair(
        &context,
        lower_token_id(),
        higher_token_id(),
        PairReadSnapshots {
            pool: &pool,
            first_token_definition: &first_definition,
            second_token_definition: &second_definition,
            first_token_vault: &first_vault,
            second_token_vault: &second_vault,
            liquidity_definition: &liquidity_definition,
            lp_lock_holding: &lp_lock,
            current_tick: &current_tick,
            clock: &clock,
        },
    )
    .expect("active pair must validate");

    let PairInspection::Active(active) = inspected else {
        panic!("initialized pool must inspect as active");
    };
    assert_eq!(active.caller_order(), PairOrder::Reversed);
    assert_eq!(
        active.pool().pool().definition_token_a_id,
        higher_token_id()
    );
    assert_eq!(active.pool().vault_a().balance(), 1_100);
    assert_eq!(active.pool().vault_b().balance(), 550);
    assert_eq!(active.pool().pool().liquidity_pool_supply, 2_000);
    assert_eq!(active.pool().pool().fees, FEE_TIER_BPS_30);
    assert_eq!(active.lp_lock_holding().balance(), 1_000);
    assert_eq!(active.stored_spot_price_q64_64(), (1u128 << 64) / 2);
    assert_eq!(active.current_tick().tick, -1);

    let donated_lp_lock = fungible_holding(
        manifest.lp_lock_holding_id(),
        TOKEN_PROGRAM_ID,
        lp_id,
        1_001,
    );
    let donated = inspect_pair(
        &context,
        lower_token_id(),
        higher_token_id(),
        PairReadSnapshots {
            pool: &pool,
            first_token_definition: &first_definition,
            second_token_definition: &second_definition,
            first_token_vault: &first_vault,
            second_token_vault: &second_vault,
            liquidity_definition: &liquidity_definition,
            lp_lock_holding: &donated_lp_lock,
            current_tick: &current_tick,
            clock: &clock,
        },
    )
    .expect("LP donated to the lock holding must not invalidate the pool");
    let PairInspection::Active(donated) = donated else {
        panic!("initialized pool with extra locked LP must remain active");
    };
    assert_eq!(donated.lp_lock_holding().balance(), 1_001);

    let wrong_lp_lock =
        fungible_holding(manifest.lp_lock_holding_id(), TOKEN_PROGRAM_ID, lp_id, 999);
    let error = inspect_pair(
        &context,
        lower_token_id(),
        higher_token_id(),
        PairReadSnapshots {
            pool: &pool,
            first_token_definition: &first_definition,
            second_token_definition: &second_definition,
            first_token_vault: &first_vault,
            second_token_vault: &second_vault,
            liquidity_definition: &liquidity_definition,
            lp_lock_holding: &wrong_lp_lock,
            current_tick: &current_tick,
            clock: &clock,
        },
    )
    .err()
    .expect("active pool must retain permanently locked minimum liquidity");
    assert!(matches!(
        error,
        amm_client::ClientError::InvalidAccountData {
            account: "LP lock holding",
            ..
        }
    ));
}

#[test]
fn missing_pair_rejects_initialized_lp_dependency() {
    let context = inspect_config(AMM_PROGRAM_ID, &config_snapshot()).expect("config must validate");
    let manifest = derive_pair_read_manifest(&context, lower_token_id(), higher_token_id())
        .expect("pair must derive");
    let pool = AccountSnapshot::new(manifest.pool_id(), Account::default());
    let first_definition = fungible_definition(lower_token_id(), 10_000, None);
    let second_definition = fungible_definition(higher_token_id(), 20_000, None);
    let first_vault = AccountSnapshot::new(manifest.first_token().vault_id(), Account::default());
    let second_vault = AccountSnapshot::new(manifest.second_token().vault_id(), Account::default());
    let lp_id = manifest.liquidity_definition_id();
    let liquidity_definition = fungible_definition(lp_id, 0, Some(lp_id));
    let lp_lock = AccountSnapshot::new(manifest.lp_lock_holding_id(), Account::default());
    let current_tick = AccountSnapshot::new(manifest.current_tick_id(), Account::default());
    let clock = clock_snapshot(manifest.clock_id());

    let error = inspect_pair(
        &context,
        lower_token_id(),
        higher_token_id(),
        PairReadSnapshots {
            pool: &pool,
            first_token_definition: &first_definition,
            second_token_definition: &second_definition,
            first_token_vault: &first_vault,
            second_token_vault: &second_vault,
            liquidity_definition: &liquidity_definition,
            lp_lock_holding: &lp_lock,
            current_tick: &current_tick,
            clock: &clock,
        },
    )
    .err()
    .expect("Token Program requires LP definition to be uninitialized");

    assert_eq!(error.code(), "invalid_account_data");
}
