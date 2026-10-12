#![cfg(test)]
#![allow(
    clippy::arithmetic_side_effects,
    clippy::integer_division,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    clippy::wildcard_enum_match_arm,
    reason = "fixtures use fixed values to lock AMM math boundaries, index fixed-length call lists, assert panics deliberately, and match one chained-call variant against a borsh-only instruction enum"
)]

//! Unit tests for the AMM's two phases.
//!
//! Each instruction is now two testable pieces rather than one: a `plan_*`
//! function over `AccountMeta`s, and one or more apply halves over a single
//! shard's bytes. The split is visible in the test names — `plan_*` tests drive
//! the first, `pool_after_*` / `check_*` tests the second.
//!
//! Three families of v0.2.5 test have no successor, each because the thing they
//! asserted no longer exists:
//!
//! * `returns_*_post_states_unchanged` / `*_is_echoed` — a handler no longer returns a diff per
//!   declared account. "Unchanged" is the *absence* of an effect, so there is nothing to assert on;
//!   a guard's apply returning `None` is the surviving form of the claim, and the runtime rejects a
//!   guard that writes.
//! * `config_not_owned_by_amm_panics` / `*_owned_by_foreign_program_panics` — a shard is keyed by
//!   the program that wrote it, so provenance is structural. Emitting an effect on `shards[self]`
//!   *is* the old ownership assertion, and `SpelOutput::guard` refuses a slot that does not select
//!   the program it names.
//! * `*_vault_balance_below_reserve_panics` (outside `SyncReserves`) — the vault reads those tests
//!   exercised are gone, not relocated: only this program can debit a vault. `SyncReserves` still
//!   reads them and keeps its versions.
//!
//! What replaces them is a new family: every value that used to be read is now
//! declared, so each one has a test that it is *pinned* — `check_config`,
//! `check_pool`, `check_vault_balance`, `check_oracle_tick`, and the bounds
//! inside each `pool_after_*`.

use std::num::NonZeroU128;

use amm_core::{
    check_config, check_vault_balance, compute_config_pda, compute_liquidity_token_pda,
    compute_lp_lock_holding_pda, compute_pool_pda, compute_protocol_fee_pda, compute_vault_pda,
    isqrt_product, mul_div_floor, protocol_fee_amount, quote_add_liquidity, quote_remove_liquidity,
    spot_price_q64_64, swap_exact_in_amounts, swap_exact_out_amounts, AddLiquidity, AmmConfig,
    CheckPoolPrice, CheckVaultBalance, ConfigFacts, PoolDefinition, PoolIdentity, RemoveLiquidity,
    RotateAuthority, SwapBinding, SyncReserves, FEE_BPS_DENOMINATOR, FEE_TIER_BPS_30,
    MINIMUM_LIQUIDITY,
};
use clock_core::{clock_account_id, CLOCK_01_PROGRAM_ACCOUNT_ID};
use lee_core::{
    account::{AccountId, ShardData},
    program::AccountMeta,
};
use token_core::{TokenDescriptor, TokenHolding, TokenKind};
use twap_oracle_core::{
    compute_current_tick_account_pda, compute_oracle_price_account_pda,
    compute_price_observations_pda, CurrentTickAccount, OBSERVATIONS_CAPACITY,
};

use crate::{
    add::{add_liquidity, plan_add_liquidity},
    create_oracle_price_account::{check_pool_price, plan_create_oracle_price_account},
    create_price_observations::{check_oracle_tick, plan_create_price_observations},
    initialize::plan_initialize,
    new_definition::plan_new_definition,
    remove::{plan_remove_liquidity, remove_liquidity},
    shared::{assert_canonical_clock, create, pool_of},
    swap::{plan_swap, swap_exact_in, swap_exact_out},
    sync::{plan_sync_reserves, sync_reserves},
    update_config::{plan_update_config, rotate_authority},
    withdraw_protocol_fees::plan_withdraw_protocol_fees,
};

// ─── fixtures ───────────────────────────────────────────────────────────────

const AMM_PROGRAM_ID: AccountId = AccountId::new([42u8; 32]);
const TOKEN_PROGRAM_ID: AccountId = AccountId::new([15u8; 32]);
const TWAP_ORACLE_PROGRAM_ID: AccountId = AccountId::new([77u8; 32]);
/// Stands in for any program that is not the one a slot is supposed to name.
const FOREIGN_PROGRAM_ID: AccountId = AccountId::new([99u8; 32]);

/// The canonical test namespace: this owner, and the default all-zero nonce.
const NONCE: [u8; 32] = [0; 32];
const NOW: u64 = 1_700_000_000_000;
const WINDOW: u64 = 3_600_000;
const SWAP_FEE_BPS: u128 = FEE_TIER_BPS_30;
/// Half the swap fee goes to the protocol.
const PROTOCOL_FEE_BPS: u128 = 5_000;

const RESERVE_A: u128 = 5_000;
const RESERVE_B: u128 = 10_000;
const LP_SUPPLY: u128 = 7_071;

fn owner_id() -> AccountId {
    AccountId::new([1u8; 32])
}
fn authority_id() -> AccountId {
    AccountId::new([2u8; 32])
}
fn other_authority_id() -> AccountId {
    AccountId::new([3u8; 32])
}
fn definition_a() -> AccountId {
    AccountId::new([10u8; 32])
}
fn definition_b() -> AccountId {
    AccountId::new([11u8; 32])
}
fn definition_c() -> AccountId {
    AccountId::new([12u8; 32])
}
fn user_a_id() -> AccountId {
    AccountId::new([20u8; 32])
}
fn user_b_id() -> AccountId {
    AccountId::new([21u8; 32])
}
fn user_lp_id() -> AccountId {
    AccountId::new([22u8; 32])
}
fn destination_id() -> AccountId {
    AccountId::new([23u8; 32])
}

fn config_id() -> AccountId {
    compute_config_pda(AMM_PROGRAM_ID, owner_id(), NONCE)
}
fn pool_id() -> AccountId {
    compute_pool_pda(AMM_PROGRAM_ID, config_id(), definition_a(), definition_b())
}
fn vault_a_id() -> AccountId {
    compute_vault_pda(AMM_PROGRAM_ID, pool_id(), definition_a())
}
fn vault_b_id() -> AccountId {
    compute_vault_pda(AMM_PROGRAM_ID, pool_id(), definition_b())
}
fn lp_definition_id() -> AccountId {
    compute_liquidity_token_pda(AMM_PROGRAM_ID, pool_id())
}
fn lp_lock_id() -> AccountId {
    compute_lp_lock_holding_pda(AMM_PROGRAM_ID, pool_id())
}
fn current_tick_id() -> AccountId {
    compute_current_tick_account_pda(TWAP_ORACLE_PROGRAM_ID, pool_id())
}
fn protocol_fee_id(definition_id: AccountId) -> AccountId {
    compute_protocol_fee_pda(AMM_PROGRAM_ID, config_id(), definition_id)
}

fn facts() -> ConfigFacts {
    ConfigFacts {
        token_program_id: TOKEN_PROGRAM_ID,
        twap_oracle_program_id: TWAP_ORACLE_PROGRAM_ID,
        authority: authority_id(),
        swap_fee_bps: SWAP_FEE_BPS,
        protocol_fee_bps: PROTOCOL_FEE_BPS,
    }
}

fn config_data() -> ShardData {
    ShardData::from(&AmmConfig {
        token_program_id: TOKEN_PROGRAM_ID,
        twap_oracle_program_id: TWAP_ORACLE_PROGRAM_ID,
        authority: authority_id(),
        swap_fee_bps: SWAP_FEE_BPS,
        protocol_fee_bps: PROTOCOL_FEE_BPS,
    })
}

fn identity() -> PoolIdentity {
    PoolIdentity {
        definition_token_a_id: definition_a(),
        definition_token_b_id: definition_b(),
        vault_a_id: vault_a_id(),
        vault_b_id: vault_b_id(),
        liquidity_pool_id: lp_definition_id(),
    }
}

fn pool(reserve_a: u128, reserve_b: u128, liquidity_pool_supply: u128) -> PoolDefinition {
    PoolDefinition {
        definition_token_a_id: definition_a(),
        definition_token_b_id: definition_b(),
        vault_a_id: vault_a_id(),
        vault_b_id: vault_b_id(),
        liquidity_pool_id: lp_definition_id(),
        liquidity_pool_supply,
        reserve_a,
        reserve_b,
    }
}

fn live_pool() -> PoolDefinition {
    pool(RESERVE_A, RESERVE_B, LP_SUPPLY)
}

fn pool_data(p: &PoolDefinition) -> ShardData {
    ShardData::from(p)
}

fn empty() -> ShardData {
    ShardData::default()
}

fn holding(definition_id: AccountId, balance: u128) -> ShardData {
    ShardData::from(&TokenHolding::Fungible {
        definition_id,
        balance,
    })
}

fn descriptor(definition_id: AccountId) -> TokenDescriptor {
    TokenDescriptor {
        definition_id,
        kind: TokenKind::Fungible,
    }
}

/// A slot selecting one of this program's own shards.
fn ours(account_id: AccountId) -> AccountMeta {
    AccountMeta::new(account_id, false, AMM_PROGRAM_ID)
}
/// A slot selecting a Token Program shard.
fn token_slot(account_id: AccountId) -> AccountMeta {
    AccountMeta::new(account_id, false, TOKEN_PROGRAM_ID)
}
/// A signed slot selecting a Token Program shard.
fn signed_token_slot(account_id: AccountId) -> AccountMeta {
    AccountMeta::new(account_id, true, TOKEN_PROGRAM_ID)
}
/// A slot selecting a TWAP oracle shard.
fn oracle_slot(account_id: AccountId) -> AccountMeta {
    AccountMeta::new(account_id, false, TWAP_ORACLE_PROGRAM_ID)
}
fn clock_slot() -> AccountMeta {
    AccountMeta::new(CLOCK_01_PROGRAM_ACCOUNT_ID, false, clock_account_id())
}

// ─── PDA derivation ─────────────────────────────────────────────────────────

#[test]
fn pool_pda_is_unique_per_token_pair_and_order_insensitive() {
    // The seed sorts the pair, so one pool serves a pair regardless of the order
    // it is named in — but a different pair is a different pool.
    let ab = compute_pool_pda(AMM_PROGRAM_ID, config_id(), definition_a(), definition_b());
    let ba = compute_pool_pda(AMM_PROGRAM_ID, config_id(), definition_b(), definition_a());
    let ac = compute_pool_pda(AMM_PROGRAM_ID, config_id(), definition_a(), definition_c());
    assert_eq!(ab, ba);
    assert_ne!(ab, ac);
}

#[test]
fn pool_pda_is_unique_per_config_namespace() {
    let other_config = compute_config_pda(AMM_PROGRAM_ID, owner_id(), [7u8; 32]);
    assert_ne!(
        compute_pool_pda(AMM_PROGRAM_ID, config_id(), definition_a(), definition_b()),
        compute_pool_pda(AMM_PROGRAM_ID, other_config, definition_a(), definition_b()),
    );
}

// ─── Initialize ─────────────────────────────────────────────────────────────

#[test]
fn plan_initialize_builds_the_config_record() {
    let data = plan_initialize(
        &AccountMeta::new(owner_id(), true, AMM_PROGRAM_ID),
        &ours(config_id()),
        NONCE,
        TOKEN_PROGRAM_ID,
        TWAP_ORACLE_PROGRAM_ID,
        authority_id(),
        SWAP_FEE_BPS,
        PROTOCOL_FEE_BPS,
        AMM_PROGRAM_ID,
    );
    let config = AmmConfig::try_from(&data).unwrap();
    assert_eq!(config.token_program_id, TOKEN_PROGRAM_ID);
    assert_eq!(config.twap_oracle_program_id, TWAP_ORACLE_PROGRAM_ID);
    assert_eq!(config.authority, authority_id());
    assert_eq!(config.swap_fee_bps, SWAP_FEE_BPS);
    assert_eq!(config.protocol_fee_bps, PROTOCOL_FEE_BPS);
}

#[test]
fn plan_initialize_accepts_any_fee_below_one_hundred_percent() {
    for fee in [0, 1, 5, 30, 100, FEE_BPS_DENOMINATOR - 1] {
        let data = plan_initialize(
            &AccountMeta::new(owner_id(), true, AMM_PROGRAM_ID),
            &ours(config_id()),
            NONCE,
            TOKEN_PROGRAM_ID,
            TWAP_ORACLE_PROGRAM_ID,
            authority_id(),
            fee,
            0,
            AMM_PROGRAM_ID,
        );
        assert_eq!(AmmConfig::try_from(&data).unwrap().swap_fee_bps, fee);
    }
}

#[test]
#[should_panic(expected = "owner account must sign")]
fn plan_initialize_rejects_an_unauthorized_owner() {
    let _ = plan_initialize(
        &AccountMeta::new(owner_id(), false, AMM_PROGRAM_ID),
        &ours(config_id()),
        NONCE,
        TOKEN_PROGRAM_ID,
        TWAP_ORACLE_PROGRAM_ID,
        authority_id(),
        SWAP_FEE_BPS,
        PROTOCOL_FEE_BPS,
        AMM_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "does not match namespaced PDA")]
fn plan_initialize_rejects_a_config_outside_the_namespace() {
    let _ = plan_initialize(
        &AccountMeta::new(owner_id(), true, AMM_PROGRAM_ID),
        &ours(AccountId::new([0xAAu8; 32])),
        NONCE,
        TOKEN_PROGRAM_ID,
        TWAP_ORACLE_PROGRAM_ID,
        authority_id(),
        SWAP_FEE_BPS,
        PROTOCOL_FEE_BPS,
        AMM_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "Swap fee must be below")]
fn plan_initialize_rejects_a_swap_fee_at_one_hundred_percent() {
    let _ = plan_initialize(
        &AccountMeta::new(owner_id(), true, AMM_PROGRAM_ID),
        &ours(config_id()),
        NONCE,
        TOKEN_PROGRAM_ID,
        TWAP_ORACLE_PROGRAM_ID,
        authority_id(),
        FEE_BPS_DENOMINATOR,
        0,
        AMM_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "Protocol fee must be at most")]
fn plan_initialize_rejects_a_protocol_fee_above_the_whole_swap_fee() {
    let _ = plan_initialize(
        &AccountMeta::new(owner_id(), true, AMM_PROGRAM_ID),
        &ours(config_id()),
        NONCE,
        TOKEN_PROGRAM_ID,
        TWAP_ORACLE_PROGRAM_ID,
        authority_id(),
        SWAP_FEE_BPS,
        FEE_BPS_DENOMINATOR + 1,
        AMM_PROGRAM_ID,
    );
}

#[test]
fn a_distinct_nonce_yields_a_distinct_instance() {
    let other = compute_config_pda(AMM_PROGRAM_ID, owner_id(), [9u8; 32]);
    assert_ne!(config_id(), other);
    // And the same nonce under a different owner is a third instance.
    assert_ne!(
        config_id(),
        compute_config_pda(AMM_PROGRAM_ID, authority_id(), NONCE)
    );
}

#[test]
fn create_writes_into_an_empty_shard() {
    assert_eq!(create(&empty(), config_data()), config_data());
}

#[test]
#[should_panic(expected = "must not already hold data")]
fn create_refuses_to_write_a_populated_shard() {
    // Strict rather than `write_once`: re-running an `Initialize` or a
    // `NewDefinition` against a live account is a mistake worth reporting, even
    // when the bytes would be identical.
    let _ = create(&config_data(), config_data());
}

// ─── UpdateConfig ───────────────────────────────────────────────────────────

#[test]
fn rotate_authority_transfers_admin_control() {
    let data = rotate_authority(
        &config_data(),
        &RotateAuthority {
            expected_authority: authority_id(),
            new_authority: other_authority_id(),
        },
    );
    let config = AmmConfig::try_from(&data).unwrap();
    assert_eq!(config.authority, other_authority_id());
    // The immutable deployment parameters are untouched.
    assert_eq!(config.token_program_id, TOKEN_PROGRAM_ID);
    assert_eq!(config.twap_oracle_program_id, TWAP_ORACLE_PROGRAM_ID);
    assert_eq!(config.swap_fee_bps, SWAP_FEE_BPS);
}

#[test]
#[should_panic(expected = "not the configured admin authority")]
fn rotate_authority_rejects_a_caller_who_is_not_the_admin() {
    let _ = rotate_authority(
        &config_data(),
        &RotateAuthority {
            expected_authority: other_authority_id(),
            new_authority: other_authority_id(),
        },
    );
}

#[test]
#[should_panic(expected = "must be initialized before use")]
fn rotate_authority_rejects_an_uninitialized_config() {
    let _ = rotate_authority(
        &empty(),
        &RotateAuthority {
            expected_authority: authority_id(),
            new_authority: other_authority_id(),
        },
    );
}

#[test]
#[should_panic(expected = "must authorize the update")]
fn plan_update_config_rejects_an_unauthorized_admin() {
    plan_update_config(&ours(authority_id()));
}

#[test]
fn plan_update_config_accepts_a_signed_admin() {
    plan_update_config(&AccountMeta::new(authority_id(), true, AMM_PROGRAM_ID));
}

// ─── the shared config guard ────────────────────────────────────────────────

#[test]
fn check_config_accepts_the_stored_facts() {
    check_config(&config_data(), &facts());
}

#[test]
#[should_panic(expected = "must be initialized before use")]
fn check_config_rejects_an_empty_config_shard() {
    check_config(&empty(), &facts());
}

#[test]
#[should_panic(expected = "Declared AMM config does not match")]
fn check_config_rejects_a_forged_token_program() {
    check_config(
        &config_data(),
        &ConfigFacts {
            token_program_id: FOREIGN_PROGRAM_ID,
            ..facts()
        },
    );
}

#[test]
#[should_panic(expected = "Declared AMM config does not match")]
fn check_config_rejects_a_forged_swap_fee() {
    // The whole fact set travels under one assertion, so a true program id
    // cannot be paired with a cheaper fee.
    check_config(
        &config_data(),
        &ConfigFacts {
            swap_fee_bps: 0,
            ..facts()
        },
    );
}

#[test]
#[should_panic(expected = "Declared AMM config does not match")]
fn check_config_rejects_a_forged_authority() {
    check_config(
        &config_data(),
        &ConfigFacts {
            authority: other_authority_id(),
            ..facts()
        },
    );
}

// ─── the shared pool guard ──────────────────────────────────────────────────

#[test]
fn pool_of_accepts_the_stored_identity() {
    let stored = pool_of(&pool_data(&live_pool()), &identity(), "test");
    assert_eq!(stored.reserve_a, RESERVE_A);
    assert_eq!(stored.reserve_b, RESERVE_B);
}

#[test]
#[should_panic(expected = "valid Pool Definition Account")]
fn pool_of_rejects_an_empty_pool_shard() {
    pool_of(&empty(), &identity(), "test");
}

#[test]
#[should_panic(expected = "Declared pool identity does not match")]
fn pool_of_rejects_a_forged_vault() {
    pool_of(
        &pool_data(&live_pool()),
        &PoolIdentity {
            vault_a_id: user_a_id(),
            ..identity()
        },
        "test",
    );
}

#[test]
#[should_panic(expected = "Declared pool identity does not match")]
fn pool_of_rejects_a_forged_lp_definition() {
    pool_of(
        &pool_data(&live_pool()),
        &PoolIdentity {
            liquidity_pool_id: user_lp_id(),
            ..identity()
        },
        "test",
    );
}

#[test]
#[should_panic(expected = "Declared pool identity does not match")]
fn pool_of_rejects_a_forged_token_pair() {
    pool_of(
        &pool_data(&live_pool()),
        &PoolIdentity {
            definition_token_b_id: definition_c(),
            ..identity()
        },
        "test",
    );
}

// ─── the clock slot ─────────────────────────────────────────────────────────

#[test]
fn assert_canonical_clock_accepts_the_clock_programs_shard() {
    assert_canonical_clock(&clock_slot(), "test");
}

#[test]
#[should_panic(expected = "canonical 1-block LEZ clock account")]
fn assert_canonical_clock_rejects_another_address() {
    assert_canonical_clock(
        &AccountMeta::new(AccountId::new([0xCCu8; 32]), false, clock_account_id()),
        "test",
    );
}

#[test]
#[should_panic(expected = "must select the clock program's shard")]
fn assert_canonical_clock_rejects_a_foreign_shard_at_the_clock_address() {
    // The address alone is not the identity: a shard is keyed by its writer, so
    // this selector would hand a guard bytes a stranger authored.
    assert_canonical_clock(
        &AccountMeta::new(CLOCK_01_PROGRAM_ACCOUNT_ID, false, FOREIGN_PROGRAM_ID),
        "test",
    );
}

// ─── helpers for the chained-call assertions ────────────────────────────────

fn observations_id() -> AccountId {
    compute_price_observations_pda(TWAP_ORACLE_PROGRAM_ID, pool_id(), WINDOW)
}
fn oracle_price_id() -> AccountId {
    compute_oracle_price_account_pda(TWAP_ORACLE_PROGRAM_ID, pool_id(), WINDOW)
}

fn selector_ids(call: &lee_core::program::ChainedCall) -> Vec<AccountId> {
    call.shard_selectors.iter().map(|s| s.account_id).collect()
}

fn oracle_instruction(call: &lee_core::program::ChainedCall) -> twap_oracle_core::Instruction {
    borsh::from_slice(&call.instruction_data)
        .expect("chained instruction must decode as a twap_oracle_core::Instruction")
}

fn token_instruction(call: &lee_core::program::ChainedCall) -> token_core::Instruction {
    borsh::from_slice(&call.instruction_data)
        .expect("chained instruction must decode as a token_core::Instruction")
}

/// `(from, to, definition_id, amount)` of a chained fungible transfer.
fn transfer_parts(
    call: &lee_core::program::ChainedCall,
) -> (AccountId, AccountId, AccountId, u128) {
    let ids = selector_ids(call);
    match token_instruction(call) {
        token_core::Instruction::Transfer {
            amount_to_transfer,
            descriptor,
        } => {
            assert_eq!(descriptor.kind, TokenKind::Fungible);
            (ids[0], ids[1], descriptor.definition_id, amount_to_transfer)
        }
        _ => panic!("expected a Transfer"),
    }
}

// ─── CreatePriceObservations ────────────────────────────────────────────────

fn plan_observations(initial_tick: i32) -> lee_core::program::ChainedCall {
    plan_create_price_observations(
        &ours(config_id()),
        &ours(pool_id()),
        &oracle_slot(current_tick_id()),
        &oracle_slot(observations_id()),
        &clock_slot(),
        WINDOW,
        &facts(),
        &identity(),
        initial_tick,
        NOW,
        AMM_PROGRAM_ID,
    )
}

#[test]
fn plan_observations_delegates_to_the_configured_oracle() {
    let call = plan_observations(-120);
    assert_eq!(call.program_account_id, TWAP_ORACLE_PROGRAM_ID);
    match oracle_instruction(&call) {
        twap_oracle_core::Instruction::CreatePriceObservations {
            initial_tick,
            window_duration,
            now,
        } => {
            assert_eq!(initial_tick, -120);
            assert_eq!(window_duration, WINDOW);
            assert_eq!(now, NOW, "the AMM forwards `now` for the oracle to pin");
        }
        _ => panic!("expected CreatePriceObservations"),
    }
}

/// `[observations, pool, clock]`, and the pool's seed is the AMM's authority.
#[test]
fn plan_observations_names_the_oracle_accounts_and_authorizes_the_pool() {
    let call = plan_observations(0);
    assert_eq!(
        selector_ids(&call),
        vec![observations_id(), pool_id(), CLOCK_01_PROGRAM_ACCOUNT_ID]
    );
    assert_eq!(
        call.pda_seeds,
        vec![amm_core::compute_pool_pda_seed(
            config_id(),
            definition_a(),
            definition_b()
        )]
    );
}

#[test]
#[should_panic(expected = "canonical 1-block LEZ clock account")]
fn plan_observations_rejects_a_non_canonical_clock() {
    let _ = plan_create_price_observations(
        &ours(config_id()),
        &ours(pool_id()),
        &oracle_slot(current_tick_id()),
        &oracle_slot(observations_id()),
        &AccountMeta::new(AccountId::new([0xCCu8; 32]), false, clock_account_id()),
        WINDOW,
        &facts(),
        &identity(),
        0,
        NOW,
        AMM_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "not derived under this config's namespace")]
fn plan_observations_rejects_a_forged_pool() {
    let _ = plan_create_price_observations(
        &ours(config_id()),
        &ours(AccountId::new([0xAAu8; 32])),
        &oracle_slot(current_tick_id()),
        &oracle_slot(observations_id()),
        &clock_slot(),
        WINDOW,
        &facts(),
        &identity(),
        0,
        NOW,
        AMM_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "current tick Account ID does not match PDA")]
fn plan_observations_rejects_a_forged_current_tick_account() {
    let _ = plan_create_price_observations(
        &ours(config_id()),
        &ours(pool_id()),
        &oracle_slot(AccountId::new([0xBBu8; 32])),
        &oracle_slot(observations_id()),
        &clock_slot(),
        WINDOW,
        &facts(),
        &identity(),
        0,
        NOW,
        AMM_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "price observations Account ID does not match PDA")]
fn plan_observations_rejects_a_wrong_observations_pda() {
    let _ = plan_create_price_observations(
        &ours(config_id()),
        &ours(pool_id()),
        &oracle_slot(current_tick_id()),
        &oracle_slot(AccountId::new([0xBBu8; 32])),
        &clock_slot(),
        WINDOW,
        &facts(),
        &identity(),
        0,
        NOW,
        AMM_PROGRAM_ID,
    );
}

#[test]
fn different_windows_produce_distinct_observation_pdas() {
    assert_ne!(
        compute_price_observations_pda(TWAP_ORACLE_PROGRAM_ID, pool_id(), WINDOW),
        compute_price_observations_pda(TWAP_ORACLE_PROGRAM_ID, pool_id(), WINDOW * 2),
    );
}

#[test]
fn check_oracle_tick_accepts_the_stored_tick() {
    let shard = ShardData::from(&CurrentTickAccount {
        tick: -120,
        last_updated: NOW,
    });
    check_oracle_tick(&shard, -120);
}

#[test]
#[should_panic(expected = "Declared initial tick does not match")]
fn check_oracle_tick_rejects_a_forged_seed_tick() {
    // The whole point of the guard: the seed is the oracle's tick, not the
    // caller's claim about it.
    let shard = ShardData::from(&CurrentTickAccount {
        tick: -120,
        last_updated: NOW,
    });
    check_oracle_tick(&shard, 5_000);
}

#[test]
#[should_panic(expected = "valid CurrentTickAccount")]
fn check_oracle_tick_rejects_an_uninitialized_current_tick_account() {
    check_oracle_tick(&empty(), 0);
}

// ─── CreateOraclePriceAccount ───────────────────────────────────────────────

fn price_guard(reserve_a: u128, reserve_b: u128) -> CheckPoolPrice {
    CheckPoolPrice {
        identity: identity(),
        reserve_a,
        reserve_b,
    }
}

fn plan_price_account(window: u64, guard: &CheckPoolPrice) -> lee_core::program::ChainedCall {
    plan_create_oracle_price_account(
        &ours(config_id()),
        &ours(pool_id()),
        &oracle_slot(compute_oracle_price_account_pda(
            TWAP_ORACLE_PROGRAM_ID,
            pool_id(),
            window,
        )),
        &clock_slot(),
        window,
        &facts(),
        guard,
        NOW,
        AMM_PROGRAM_ID,
    )
}

#[test]
fn plan_price_account_seeds_the_pair_and_spot_price_from_the_pinned_reserves() {
    let call = plan_price_account(WINDOW, &price_guard(RESERVE_A, RESERVE_B));
    assert_eq!(call.program_account_id, TWAP_ORACLE_PROGRAM_ID);
    match oracle_instruction(&call) {
        twap_oracle_core::Instruction::CreateOraclePriceAccount {
            base_asset,
            quote_asset,
            initial_price,
            window_duration,
            now,
        } => {
            assert_eq!(base_asset, definition_a());
            assert_eq!(quote_asset, definition_b());
            assert_eq!(initial_price, spot_price_q64_64(RESERVE_A, RESERVE_B));
            assert_eq!(window_duration, WINDOW);
            assert_eq!(now, NOW);
        }
        _ => panic!("expected CreateOraclePriceAccount"),
    }
    assert_eq!(
        selector_ids(&call),
        vec![oracle_price_id(), pool_id(), CLOCK_01_PROGRAM_ACCOUNT_ID]
    );
}

#[test]
#[should_panic(expected = "canonical 1-block LEZ clock account")]
fn plan_price_account_rejects_a_non_canonical_clock() {
    let _ = plan_create_oracle_price_account(
        &ours(config_id()),
        &ours(pool_id()),
        &oracle_slot(oracle_price_id()),
        &AccountMeta::new(AccountId::new([0xCCu8; 32]), false, clock_account_id()),
        WINDOW,
        &facts(),
        &price_guard(RESERVE_A, RESERVE_B),
        NOW,
        AMM_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "not derived under this config's namespace")]
fn plan_price_account_rejects_a_forged_pool() {
    let _ = plan_create_oracle_price_account(
        &ours(config_id()),
        &ours(AccountId::new([0xAAu8; 32])),
        &oracle_slot(oracle_price_id()),
        &clock_slot(),
        WINDOW,
        &facts(),
        &price_guard(RESERVE_A, RESERVE_B),
        NOW,
        AMM_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "oracle price Account ID does not match PDA")]
fn plan_price_account_rejects_a_wrong_price_account_pda() {
    let _ = plan_create_oracle_price_account(
        &ours(config_id()),
        &ours(pool_id()),
        &oracle_slot(AccountId::new([0xBBu8; 32])),
        &clock_slot(),
        WINDOW,
        &facts(),
        &price_guard(RESERVE_A, RESERVE_B),
        NOW,
        AMM_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "window_duration must be >= OBSERVATIONS_CAPACITY")]
fn plan_price_account_rejects_a_window_below_the_observations_capacity() {
    let window = u64::from(OBSERVATIONS_CAPACITY) - 1;
    plan_price_account(window, &price_guard(RESERVE_A, RESERVE_B));
}

#[test]
fn plan_price_account_accepts_a_window_equal_to_the_observations_capacity() {
    let window = u64::from(OBSERVATIONS_CAPACITY);
    let call = plan_price_account(window, &price_guard(RESERVE_A, RESERVE_B));
    assert_eq!(call.program_account_id, TWAP_ORACLE_PROGRAM_ID);
}

#[test]
#[should_panic(expected = "spot price must be non-zero")]
fn plan_price_account_rejects_a_zero_quote_reserve() {
    plan_price_account(WINDOW, &price_guard(RESERVE_A, 0));
}

#[test]
#[should_panic(expected = "spot price must be non-zero")]
fn plan_price_account_rejects_a_quote_reserve_that_floors_the_price_to_zero() {
    // `reserve_b << 64 / reserve_a` floors to zero when the quote side is
    // negligible, and zero is the sentinel consumers read as "no price".
    plan_price_account(WINDOW, &price_guard(1u128 << 100, 1));
}

#[test]
fn different_windows_produce_distinct_price_account_pdas() {
    assert_ne!(
        compute_oracle_price_account_pda(TWAP_ORACLE_PROGRAM_ID, pool_id(), WINDOW),
        compute_oracle_price_account_pda(TWAP_ORACLE_PROGRAM_ID, pool_id(), WINDOW * 2),
    );
}

#[test]
fn check_pool_price_accepts_the_stored_reserves() {
    check_pool_price(&pool_data(&live_pool()), &price_guard(RESERVE_A, RESERVE_B));
}

#[test]
#[should_panic(expected = "Declared pool reserves do not match")]
fn check_pool_price_rejects_forged_reserves() {
    // Exact, unlike the mutating instructions' bounds: a seeded price is read by
    // consumers, so a stale-by-one-swap seed is not acceptable.
    check_pool_price(
        &pool_data(&live_pool()),
        &price_guard(RESERVE_A, RESERVE_B + 1),
    );
}

// ─── NewDefinition ──────────────────────────────────────────────────────────

struct NewDefinitionSlots {
    pool: AccountMeta,
    vault_a: AccountMeta,
    vault_b: AccountMeta,
    lp_definition: AccountMeta,
    lp_lock: AccountMeta,
    current_tick: AccountMeta,
}

impl Default for NewDefinitionSlots {
    fn default() -> Self {
        Self {
            pool: ours(pool_id()),
            vault_a: token_slot(vault_a_id()),
            vault_b: token_slot(vault_b_id()),
            lp_definition: token_slot(lp_definition_id()),
            lp_lock: token_slot(lp_lock_id()),
            current_tick: oracle_slot(current_tick_id()),
        }
    }
}

fn plan_definition(
    slots: &NewDefinitionSlots,
    token_a_amount: u128,
    token_b_amount: u128,
    definition_token_a_id: AccountId,
    definition_token_b_id: AccountId,
) -> (ShardData, Vec<lee_core::program::ChainedCall>) {
    plan_new_definition(
        &ours(config_id()),
        &slots.pool,
        &slots.vault_a,
        &slots.vault_b,
        &slots.lp_definition,
        &slots.lp_lock,
        &signed_token_slot(user_a_id()),
        &signed_token_slot(user_b_id()),
        &token_slot(user_lp_id()),
        &slots.current_tick,
        &clock_slot(),
        NonZeroU128::new(token_a_amount).unwrap(),
        NonZeroU128::new(token_b_amount).unwrap(),
        &facts(),
        definition_token_a_id,
        definition_token_b_id,
        NOW,
        AMM_PROGRAM_ID,
    )
}

fn plan_definition_ok() -> (ShardData, Vec<lee_core::program::ChainedCall>) {
    plan_definition(
        &NewDefinitionSlots::default(),
        RESERVE_A,
        RESERVE_B,
        definition_a(),
        definition_b(),
    )
}

#[test]
fn plan_new_definition_builds_the_pool_record() {
    let (data, _) = plan_definition_ok();
    let built = PoolDefinition::try_from(&data).unwrap();
    assert_eq!(built.definition_token_a_id, definition_a());
    assert_eq!(built.definition_token_b_id, definition_b());
    assert_eq!(built.vault_a_id, vault_a_id());
    assert_eq!(built.vault_b_id, vault_b_id());
    assert_eq!(built.liquidity_pool_id, lp_definition_id());
    assert_eq!(built.reserve_a, RESERVE_A);
    assert_eq!(built.reserve_b, RESERVE_B);
    assert_eq!(
        built.liquidity_pool_supply,
        isqrt_product(RESERVE_A, RESERVE_B)
    );
    // The record the pool is created with is the identity every later
    // instruction pins its declaration against.
    assert_eq!(PoolIdentity::from(&built), identity());
}

/// LP definition, LP mint, token B, token A, current-tick creation.
#[test]
fn plan_new_definition_chains_five_calls_in_order() {
    let (_, calls) = plan_definition_ok();
    assert_eq!(calls.len(), 5);

    match token_instruction(&calls[0]) {
        token_core::Instruction::NewFungibleDefinition {
            total_supply,
            mint_authority,
            ..
        } => {
            assert_eq!(total_supply, MINIMUM_LIQUIDITY, "the lock is permanent");
            assert_eq!(
                mint_authority,
                Some(lp_definition_id()),
                "the LP token is self-authoritative: only the pool can mint it"
            );
        }
        _ => panic!("expected NewFungibleDefinition"),
    }
    assert_eq!(
        selector_ids(&calls[0]),
        vec![lp_definition_id(), lp_lock_id()]
    );

    match token_instruction(&calls[1]) {
        token_core::Instruction::Mint { amount_to_mint } => assert_eq!(
            amount_to_mint,
            isqrt_product(RESERVE_A, RESERVE_B) - MINIMUM_LIQUIDITY,
            "the caller gets the initial LP minus the lock"
        ),
        _ => panic!("expected Mint"),
    }

    assert_eq!(
        transfer_parts(&calls[2]),
        (user_b_id(), vault_b_id(), definition_b(), RESERVE_B)
    );
    assert_eq!(
        transfer_parts(&calls[3]),
        (user_a_id(), vault_a_id(), definition_a(), RESERVE_A)
    );

    match oracle_instruction(&calls[4]) {
        twap_oracle_core::Instruction::CreateCurrentTickAccount { initial_price, now } => {
            assert_eq!(
                initial_price,
                spot_price_q64_64(RESERVE_A, RESERVE_B),
                "the opening tick comes from the pool's own reserves"
            );
            assert_eq!(now, NOW);
        }
        _ => panic!("expected CreateCurrentTickAccount"),
    }
}

/// Each chained transfer carries the declared pair as its descriptor, which is
/// what makes the Token Program refuse a trader holding of some other asset.
#[test]
fn plan_new_definition_pins_the_declared_pair_through_the_transfer_descriptors() {
    let (_, calls) = plan_definition_ok();
    let (_, _, definition_in_b, _) = transfer_parts(&calls[2]);
    let (_, _, definition_in_a, _) = transfer_parts(&calls[3]);
    assert_eq!(definition_in_b, definition_b());
    assert_eq!(definition_in_a, definition_a());
}

#[test]
#[should_panic(expected = "Cannot set up a swap for a token with itself")]
fn plan_new_definition_rejects_a_pair_of_one_token() {
    plan_definition(
        &NewDefinitionSlots::default(),
        RESERVE_A,
        RESERVE_B,
        definition_a(),
        definition_a(),
    );
}

#[test]
#[should_panic(expected = "Pool Definition Account ID does not match PDA")]
fn plan_new_definition_rejects_a_wrong_pool_id() {
    let _ = plan_definition(
        &NewDefinitionSlots {
            pool: ours(AccountId::new([0xAAu8; 32])),
            ..NewDefinitionSlots::default()
        },
        RESERVE_A,
        RESERVE_B,
        definition_a(),
        definition_b(),
    );
}

#[test]
#[should_panic(expected = "Vault ID does not match PDA")]
fn plan_new_definition_rejects_a_wrong_vault_a_id() {
    let _ = plan_definition(
        &NewDefinitionSlots {
            vault_a: token_slot(AccountId::new([0xAAu8; 32])),
            ..NewDefinitionSlots::default()
        },
        RESERVE_A,
        RESERVE_B,
        definition_a(),
        definition_b(),
    );
}

#[test]
#[should_panic(expected = "Vault ID does not match PDA")]
fn plan_new_definition_rejects_a_wrong_vault_b_id() {
    let _ = plan_definition(
        &NewDefinitionSlots {
            vault_b: token_slot(AccountId::new([0xAAu8; 32])),
            ..NewDefinitionSlots::default()
        },
        RESERVE_A,
        RESERVE_B,
        definition_a(),
        definition_b(),
    );
}

#[test]
#[should_panic(expected = "Liquidity pool Token Definition Account ID does not match PDA")]
fn plan_new_definition_rejects_a_wrong_lp_definition_id() {
    let _ = plan_definition(
        &NewDefinitionSlots {
            lp_definition: token_slot(AccountId::new([0xAAu8; 32])),
            ..NewDefinitionSlots::default()
        },
        RESERVE_A,
        RESERVE_B,
        definition_a(),
        definition_b(),
    );
}

#[test]
#[should_panic(expected = "LP lock holding Account ID does not match PDA")]
fn plan_new_definition_rejects_a_wrong_lp_lock_id() {
    let _ = plan_definition(
        &NewDefinitionSlots {
            lp_lock: token_slot(AccountId::new([0xAAu8; 32])),
            ..NewDefinitionSlots::default()
        },
        RESERVE_A,
        RESERVE_B,
        definition_a(),
        definition_b(),
    );
}

#[test]
#[should_panic(expected = "current tick Account ID does not match PDA")]
fn plan_new_definition_rejects_a_wrong_current_tick_id() {
    let _ = plan_definition(
        &NewDefinitionSlots {
            current_tick: oracle_slot(AccountId::new([0xAAu8; 32])),
            ..NewDefinitionSlots::default()
        },
        RESERVE_A,
        RESERVE_B,
        definition_a(),
        definition_b(),
    );
}

#[test]
#[should_panic(expected = "must exceed minimum liquidity lock")]
fn plan_new_definition_rejects_liquidity_at_the_lock_floor() {
    // isqrt(1_000 * 1_000) == MINIMUM_LIQUIDITY exactly, leaving the caller
    // nothing.
    let _ = plan_definition(
        &NewDefinitionSlots::default(),
        MINIMUM_LIQUIDITY,
        MINIMUM_LIQUIDITY,
        definition_a(),
        definition_b(),
    );
}

#[test]
fn plan_new_definition_handles_asymmetric_and_large_amounts() {
    // Asymmetric: LP is the geometric mean, not either leg.
    let (data, _) = plan_definition(
        &NewDefinitionSlots::default(),
        1_000_000,
        4,
        definition_a(),
        definition_b(),
    );
    assert_eq!(
        PoolDefinition::try_from(&data)
            .unwrap()
            .liquidity_pool_supply,
        2_000
    );

    // Realistic 18-decimal amounts: the product is widened to U256 before the
    // sqrt, so nothing overflows u128 on the way.
    let big = 1_000_000_000u128 * 10u128.pow(18);
    let (data, _) = plan_definition(
        &NewDefinitionSlots::default(),
        big,
        big,
        definition_a(),
        definition_b(),
    );
    assert_eq!(
        PoolDefinition::try_from(&data)
            .unwrap()
            .liquidity_pool_supply,
        big
    );
}

// ─── AddLiquidity ───────────────────────────────────────────────────────────

const MAX_ADD_A: u128 = 500;
const MAX_ADD_B: u128 = 1_000;

/// The declaration a caller builds from the live pool: the shared quote, plus the
/// post-add price the pool pins exactly.
fn add_effect(p: &PoolDefinition, max_a: u128, max_b: u128, min_lp: u128) -> AddLiquidity {
    let (amount_a, amount_b, delta_lp) = quote_add_liquidity(p, max_a, max_b);
    AddLiquidity {
        identity: identity(),
        new_price: spot_price_q64_64(p.reserve_a + amount_a, p.reserve_b + amount_b),
        amount_a,
        amount_b,
        delta_lp,
        min_amount_liquidity: min_lp,
        max_amount_a: max_a,
        max_amount_b: max_b,
    }
}

fn plan_add(effect: &AddLiquidity) -> Vec<lee_core::program::ChainedCall> {
    plan_add_liquidity(
        &ours(config_id()),
        &ours(pool_id()),
        &token_slot(vault_a_id()),
        &token_slot(vault_b_id()),
        &token_slot(lp_definition_id()),
        &signed_token_slot(user_a_id()),
        &signed_token_slot(user_b_id()),
        &token_slot(user_lp_id()),
        &oracle_slot(current_tick_id()),
        &clock_slot(),
        &facts(),
        effect,
        NOW,
        AMM_PROGRAM_ID,
    )
}

#[test]
fn quote_add_liquidity_matches_the_pools_ratio() {
    let (amount_a, amount_b, delta_lp) = quote_add_liquidity(&live_pool(), MAX_ADD_A, MAX_ADD_B);
    assert_eq!((amount_a, amount_b), (500, 1_000));
    assert_eq!(delta_lp, 707);
}

#[test]
fn quote_add_liquidity_is_capped_by_the_scarcer_side() {
    // Offering twice as much A as the ratio wants deposits only what B can match.
    let (amount_a, amount_b, _) = quote_add_liquidity(&live_pool(), 1_000, MAX_ADD_B);
    assert_eq!((amount_a, amount_b), (500, 1_000));
}

/// LP mint, token B, token A, tick refresh.
#[test]
fn plan_add_liquidity_chains_four_calls_in_order() {
    let effect = add_effect(&live_pool(), MAX_ADD_A, MAX_ADD_B, 1);
    let calls = plan_add(&effect);
    assert_eq!(calls.len(), 4);

    match token_instruction(&calls[0]) {
        token_core::Instruction::Mint { amount_to_mint } => {
            assert_eq!(amount_to_mint, effect.delta_lp);
        }
        _ => panic!("expected Mint"),
    }
    assert_eq!(
        selector_ids(&calls[0]),
        vec![lp_definition_id(), user_lp_id()]
    );
    assert_eq!(
        transfer_parts(&calls[1]),
        (user_b_id(), vault_b_id(), definition_b(), effect.amount_b)
    );
    assert_eq!(
        transfer_parts(&calls[2]),
        (user_a_id(), vault_a_id(), definition_a(), effect.amount_a)
    );
    match oracle_instruction(&calls[3]) {
        twap_oracle_core::Instruction::UpdateCurrentTick { price, now } => {
            assert_eq!(price, effect.new_price);
            assert_eq!(now, NOW);
        }
        _ => panic!("expected UpdateCurrentTick"),
    }
}

#[test]
#[should_panic(expected = "Vault A was not provided")]
fn plan_add_liquidity_rejects_a_wrong_vault_a_slot() {
    let effect = add_effect(&live_pool(), MAX_ADD_A, MAX_ADD_B, 1);
    let _ = plan_add_liquidity(
        &ours(config_id()),
        &ours(pool_id()),
        &token_slot(AccountId::new([0xAAu8; 32])),
        &token_slot(vault_b_id()),
        &token_slot(lp_definition_id()),
        &signed_token_slot(user_a_id()),
        &signed_token_slot(user_b_id()),
        &token_slot(user_lp_id()),
        &oracle_slot(current_tick_id()),
        &clock_slot(),
        &facts(),
        &effect,
        NOW,
        AMM_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "Vault B was not provided")]
fn plan_add_liquidity_rejects_a_wrong_vault_b_slot() {
    let effect = add_effect(&live_pool(), MAX_ADD_A, MAX_ADD_B, 1);
    let _ = plan_add_liquidity(
        &ours(config_id()),
        &ours(pool_id()),
        &token_slot(vault_a_id()),
        &token_slot(AccountId::new([0xAAu8; 32])),
        &token_slot(lp_definition_id()),
        &signed_token_slot(user_a_id()),
        &signed_token_slot(user_b_id()),
        &token_slot(user_lp_id()),
        &oracle_slot(current_tick_id()),
        &clock_slot(),
        &facts(),
        &effect,
        NOW,
        AMM_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "LP definition mismatch")]
fn plan_add_liquidity_rejects_a_wrong_lp_definition_slot() {
    let effect = add_effect(&live_pool(), MAX_ADD_A, MAX_ADD_B, 1);
    let _ = plan_add_liquidity(
        &ours(config_id()),
        &ours(pool_id()),
        &token_slot(vault_a_id()),
        &token_slot(vault_b_id()),
        &token_slot(AccountId::new([0xAAu8; 32])),
        &signed_token_slot(user_a_id()),
        &signed_token_slot(user_b_id()),
        &token_slot(user_lp_id()),
        &oracle_slot(current_tick_id()),
        &clock_slot(),
        &facts(),
        &effect,
        NOW,
        AMM_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "not derived under this config's namespace")]
fn plan_add_liquidity_rejects_a_pool_outside_the_config_namespace() {
    let effect = add_effect(&live_pool(), MAX_ADD_A, MAX_ADD_B, 1);
    let _ = plan_add_liquidity(
        &ours(config_id()),
        &ours(AccountId::new([0xAAu8; 32])),
        &token_slot(vault_a_id()),
        &token_slot(vault_b_id()),
        &token_slot(lp_definition_id()),
        &signed_token_slot(user_a_id()),
        &signed_token_slot(user_b_id()),
        &token_slot(user_lp_id()),
        &oracle_slot(current_tick_id()),
        &clock_slot(),
        &facts(),
        &effect,
        NOW,
        AMM_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "Both max-balances must be nonzero")]
fn plan_add_liquidity_rejects_a_zero_max_a() {
    let mut effect = add_effect(&live_pool(), MAX_ADD_A, MAX_ADD_B, 1);
    effect.max_amount_a = 0;
    let _ = plan_add(&effect);
}

#[test]
#[should_panic(expected = "Both max-balances must be nonzero")]
fn plan_add_liquidity_rejects_a_zero_max_b() {
    let mut effect = add_effect(&live_pool(), MAX_ADD_A, MAX_ADD_B, 1);
    effect.max_amount_b = 0;
    let _ = plan_add(&effect);
}

#[test]
fn pool_after_add_records_the_deposit() {
    let p = live_pool();
    let effect = add_effect(&p, MAX_ADD_A, MAX_ADD_B, 707);
    let post = PoolDefinition::try_from(&add_liquidity(&pool_data(&p), &effect)).unwrap();
    assert_eq!(post.reserve_a, RESERVE_A + 500);
    assert_eq!(post.reserve_b, RESERVE_B + 1_000);
    assert_eq!(post.liquidity_pool_supply, LP_SUPPLY + 707);
    // The identity is carried through untouched.
    assert_eq!(PoolIdentity::from(&post), identity());
}

/// The bound is one-directional on purpose: under-claiming LP leaves the surplus
/// with the existing providers, so it is accepted.
#[test]
fn pool_after_add_accepts_less_lp_than_the_deposit_bought() {
    let p = live_pool();
    let mut effect = add_effect(&p, MAX_ADD_A, MAX_ADD_B, 1);
    effect.delta_lp -= 100;
    let post = PoolDefinition::try_from(&add_liquidity(&pool_data(&p), &effect)).unwrap();
    assert_eq!(post.liquidity_pool_supply, LP_SUPPLY + 607);
}

#[test]
#[should_panic(expected = "exceeds the pro-rata share of the deposit")]
fn pool_after_add_rejects_more_lp_than_the_deposit_bought() {
    let p = live_pool();
    let mut effect = add_effect(&p, MAX_ADD_A, MAX_ADD_B, 1);
    effect.delta_lp += 1;
    let _ = add_liquidity(&pool_data(&p), &effect);
}

/// The reason the bound checks *both* sides: a lopsided deposit must be priced by
/// its scarcer leg, or one token could buy LP against the other's reserve.
#[test]
#[should_panic(expected = "exceeds the pro-rata share of the deposit")]
fn pool_after_add_prices_a_lopsided_deposit_by_its_scarcer_side() {
    let p = live_pool();
    let effect = AddLiquidity {
        identity: identity(),
        new_price: spot_price_q64_64(RESERVE_A + 500, RESERVE_B + 1),
        amount_a: 500,
        amount_b: 1,
        // What side A alone would have bought.
        delta_lp: 707,
        min_amount_liquidity: 1,
        max_amount_a: 500,
        max_amount_b: 1,
    };
    let _ = add_liquidity(&pool_data(&p), &effect);
}

#[test]
#[should_panic(expected = "Payable LP is less than provided minimum")]
fn pool_after_add_rejects_lp_below_the_callers_floor() {
    let p = live_pool();
    let mut effect = add_effect(&p, MAX_ADD_A, MAX_ADD_B, 708);
    effect.delta_lp = 707;
    let _ = add_liquidity(&pool_data(&p), &effect);
}

#[test]
#[should_panic(expected = "Actual trade amounts cannot exceed max_amounts")]
fn pool_after_add_rejects_a_deposit_above_the_callers_ceiling() {
    let p = live_pool();
    let mut effect = add_effect(&p, MAX_ADD_A, MAX_ADD_B, 1);
    effect.amount_a += 1;
    let _ = add_liquidity(&pool_data(&p), &effect);
}

#[test]
#[should_panic(expected = "Payable LP must be nonzero")]
fn pool_after_add_rejects_a_zero_lp_mint() {
    let p = live_pool();
    let mut effect = add_effect(&p, MAX_ADD_A, MAX_ADD_B, 0);
    effect.delta_lp = 0;
    let _ = add_liquidity(&pool_data(&p), &effect);
}

#[test]
#[should_panic(expected = "A trade amount is 0")]
fn pool_after_add_rejects_a_zero_deposit() {
    let p = live_pool();
    let mut effect = add_effect(&p, MAX_ADD_A, MAX_ADD_B, 1);
    effect.amount_b = 0;
    let _ = add_liquidity(&pool_data(&p), &effect);
}

#[test]
#[should_panic(expected = "Reserves must be nonzero")]
fn pool_after_add_rejects_a_pool_with_a_zero_reserve() {
    let effect = add_effect(&live_pool(), MAX_ADD_A, MAX_ADD_B, 1);
    let _ = add_liquidity(&pool_data(&pool(0, RESERVE_B, LP_SUPPLY)), &effect);
}

#[test]
#[should_panic(expected = "Declared post-add price does not match")]
fn pool_after_add_rejects_a_forged_post_price() {
    let p = live_pool();
    let mut effect = add_effect(&p, MAX_ADD_A, MAX_ADD_B, 1);
    effect.new_price += 1;
    let _ = add_liquidity(&pool_data(&p), &effect);
}

/// The price is pinned exactly, so a pool that moved between the quote and the
/// add is a retry rather than a stale tick.
#[test]
#[should_panic(expected = "Declared post-add price does not match")]
fn pool_after_add_rejects_a_declaration_quoted_against_stale_reserves() {
    let effect = add_effect(&live_pool(), MAX_ADD_A, MAX_ADD_B, 1);
    // A swap landed first: same identity, and reserves that still satisfy the LP
    // bound (a smaller input reserve buys *more* LP per unit), but a ratio the
    // declared price no longer describes.
    let _ = add_liquidity(
        &pool_data(&pool(RESERVE_A - 50, RESERVE_B, LP_SUPPLY)),
        &effect,
    );
}

#[test]
#[should_panic(expected = "Declared pool identity does not match")]
fn pool_after_add_rejects_a_forged_identity() {
    let p = live_pool();
    let mut effect = add_effect(&p, MAX_ADD_A, MAX_ADD_B, 1);
    effect.identity.vault_a_id = user_a_id();
    let _ = add_liquidity(&pool_data(&p), &effect);
}

#[test]
fn pool_after_add_survives_realistic_eighteen_decimal_amounts() {
    let big = 1_000_000u128 * 10u128.pow(18);
    let p = pool(big, big, big);
    let effect = add_effect(&p, big / 2, big / 2, 1);
    let post = PoolDefinition::try_from(&add_liquidity(&pool_data(&p), &effect)).unwrap();
    assert_eq!(post.reserve_a, big + big / 2);
    assert_eq!(post.liquidity_pool_supply, big + big / 2);
}

// ─── RemoveLiquidity ────────────────────────────────────────────────────────

const REMOVE_LP: u128 = 707;

fn remove_effect(p: &PoolDefinition, remove: u128, min_a: u128, min_b: u128) -> RemoveLiquidity {
    let (withdraw_amount_a, withdraw_amount_b) = quote_remove_liquidity(p, remove);
    RemoveLiquidity {
        identity: identity(),
        new_price: spot_price_q64_64(
            p.reserve_a - withdraw_amount_a,
            p.reserve_b - withdraw_amount_b,
        ),
        remove_liquidity_amount: remove,
        withdraw_amount_a,
        withdraw_amount_b,
        min_amount_to_remove_token_a: min_a,
        min_amount_to_remove_token_b: min_b,
    }
}

fn plan_remove(effect: &RemoveLiquidity) -> Vec<lee_core::program::ChainedCall> {
    plan_remove_liquidity(
        &ours(config_id()),
        &ours(pool_id()),
        &token_slot(vault_a_id()),
        &token_slot(vault_b_id()),
        &token_slot(lp_definition_id()),
        &token_slot(user_a_id()),
        &token_slot(user_b_id()),
        &signed_token_slot(user_lp_id()),
        &oracle_slot(current_tick_id()),
        &clock_slot(),
        &facts(),
        effect,
        NOW,
        AMM_PROGRAM_ID,
    )
}

#[test]
fn quote_remove_liquidity_is_the_pro_rata_share() {
    let (a, b) = quote_remove_liquidity(&live_pool(), REMOVE_LP);
    assert_eq!((a, b), (499, 999)); // floored
}

/// LP burn, token B, token A, tick refresh. Each withdrawal rides on its vault's
/// seed, which is the whole of the AMM's authority over that vault.
#[test]
fn plan_remove_liquidity_chains_four_calls_in_order() {
    let effect = remove_effect(&live_pool(), REMOVE_LP, 1, 1);
    let calls = plan_remove(&effect);
    assert_eq!(calls.len(), 4);

    match token_instruction(&calls[0]) {
        token_core::Instruction::Burn {
            amount_to_burn,
            kind,
        } => {
            assert_eq!(amount_to_burn, REMOVE_LP);
            assert_eq!(kind, TokenKind::Fungible);
        }
        _ => panic!("expected Burn"),
    }
    // The burn names the LP definition first, which is what pins the burner's
    // holding to the pool's own LP token.
    assert_eq!(
        selector_ids(&calls[0]),
        vec![lp_definition_id(), user_lp_id()]
    );

    assert_eq!(
        transfer_parts(&calls[1]),
        (
            vault_b_id(),
            user_b_id(),
            definition_b(),
            effect.withdraw_amount_b
        )
    );
    assert_eq!(
        calls[1].pda_seeds,
        vec![amm_core::compute_vault_pda_seed(pool_id(), definition_b())]
    );
    assert_eq!(
        transfer_parts(&calls[2]),
        (
            vault_a_id(),
            user_a_id(),
            definition_a(),
            effect.withdraw_amount_a
        )
    );
    assert_eq!(
        calls[2].pda_seeds,
        vec![amm_core::compute_vault_pda_seed(pool_id(), definition_a())]
    );

    match oracle_instruction(&calls[3]) {
        twap_oracle_core::Instruction::UpdateCurrentTick { price, now } => {
            assert_eq!(price, effect.new_price);
            assert_eq!(now, NOW);
        }
        _ => panic!("expected UpdateCurrentTick"),
    }
}

#[test]
#[should_panic(expected = "Vault A was not provided")]
fn plan_remove_liquidity_rejects_a_wrong_vault_a_slot() {
    let effect = remove_effect(&live_pool(), REMOVE_LP, 1, 1);
    let _ = plan_remove_liquidity(
        &ours(config_id()),
        &ours(pool_id()),
        &token_slot(AccountId::new([0xAAu8; 32])),
        &token_slot(vault_b_id()),
        &token_slot(lp_definition_id()),
        &token_slot(user_a_id()),
        &token_slot(user_b_id()),
        &signed_token_slot(user_lp_id()),
        &oracle_slot(current_tick_id()),
        &clock_slot(),
        &facts(),
        &effect,
        NOW,
        AMM_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "Vault B was not provided")]
fn plan_remove_liquidity_rejects_a_wrong_vault_b_slot() {
    let effect = remove_effect(&live_pool(), REMOVE_LP, 1, 1);
    let _ = plan_remove_liquidity(
        &ours(config_id()),
        &ours(pool_id()),
        &token_slot(vault_a_id()),
        &token_slot(AccountId::new([0xAAu8; 32])),
        &token_slot(lp_definition_id()),
        &token_slot(user_a_id()),
        &token_slot(user_b_id()),
        &signed_token_slot(user_lp_id()),
        &oracle_slot(current_tick_id()),
        &clock_slot(),
        &facts(),
        &effect,
        NOW,
        AMM_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "LP definition mismatch")]
fn plan_remove_liquidity_rejects_a_wrong_lp_definition_slot() {
    let effect = remove_effect(&live_pool(), REMOVE_LP, 1, 1);
    let _ = plan_remove_liquidity(
        &ours(config_id()),
        &ours(pool_id()),
        &token_slot(vault_a_id()),
        &token_slot(vault_b_id()),
        &token_slot(AccountId::new([0xAAu8; 32])),
        &token_slot(user_a_id()),
        &token_slot(user_b_id()),
        &signed_token_slot(user_lp_id()),
        &oracle_slot(current_tick_id()),
        &clock_slot(),
        &facts(),
        &effect,
        NOW,
        AMM_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "not derived under this config's namespace")]
fn plan_remove_liquidity_rejects_a_pool_outside_the_config_namespace() {
    let effect = remove_effect(&live_pool(), REMOVE_LP, 1, 1);
    let _ = plan_remove_liquidity(
        &ours(config_id()),
        &ours(AccountId::new([0xAAu8; 32])),
        &token_slot(vault_a_id()),
        &token_slot(vault_b_id()),
        &token_slot(lp_definition_id()),
        &token_slot(user_a_id()),
        &token_slot(user_b_id()),
        &signed_token_slot(user_lp_id()),
        &oracle_slot(current_tick_id()),
        &clock_slot(),
        &facts(),
        &effect,
        NOW,
        AMM_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "Minimum withdraw amount must be nonzero")]
fn plan_remove_liquidity_rejects_a_zero_minimum_for_token_a() {
    let effect = remove_effect(&live_pool(), REMOVE_LP, 0, 1);
    let _ = plan_remove(&effect);
}

#[test]
#[should_panic(expected = "Minimum withdraw amount must be nonzero")]
fn plan_remove_liquidity_rejects_a_zero_minimum_for_token_b() {
    let effect = remove_effect(&live_pool(), REMOVE_LP, 1, 0);
    let _ = plan_remove(&effect);
}

#[test]
fn pool_after_remove_records_the_withdrawal() {
    let p = live_pool();
    let effect = remove_effect(&p, REMOVE_LP, 1, 1);
    let post = PoolDefinition::try_from(&remove_liquidity(&pool_data(&p), &effect)).unwrap();
    assert_eq!(post.reserve_a, RESERVE_A - 499);
    assert_eq!(post.reserve_b, RESERVE_B - 999);
    assert_eq!(post.liquidity_pool_supply, LP_SUPPLY - REMOVE_LP);
}

/// One-directional on purpose: withdrawing less than the share leaves the
/// surplus with the remaining providers.
#[test]
fn pool_after_remove_accepts_less_than_the_pro_rata_share() {
    let p = live_pool();
    let mut effect = remove_effect(&p, REMOVE_LP, 1, 1);
    effect.withdraw_amount_a -= 10;
    effect.new_price = spot_price_q64_64(
        RESERVE_A - effect.withdraw_amount_a,
        RESERVE_B - effect.withdraw_amount_b,
    );
    let post = PoolDefinition::try_from(&remove_liquidity(&pool_data(&p), &effect)).unwrap();
    assert_eq!(post.reserve_a, RESERVE_A - 489);
}

#[test]
#[should_panic(expected = "Withdrawal (Token A) exceeds the burner's pro-rata share")]
fn pool_after_remove_rejects_more_than_the_pro_rata_share() {
    let p = live_pool();
    let mut effect = remove_effect(&p, REMOVE_LP, 1, 1);
    effect.withdraw_amount_a += 1;
    effect.new_price = spot_price_q64_64(
        RESERVE_A - effect.withdraw_amount_a,
        RESERVE_B - effect.withdraw_amount_b,
    );
    let _ = remove_liquidity(&pool_data(&p), &effect);
}

#[test]
#[should_panic(expected = "Insufficient minimal withdraw amount (Token B)")]
fn pool_after_remove_rejects_a_withdrawal_below_the_callers_floor() {
    let p = live_pool();
    let effect = remove_effect(&p, REMOVE_LP, 1, 1_000);
    let _ = remove_liquidity(&pool_data(&p), &effect);
}

#[test]
#[should_panic(expected = "Pool liquidity supply is below minimum liquidity")]
fn pool_after_remove_rejects_a_pool_below_the_lock_floor() {
    let p = pool(RESERVE_A, RESERVE_B, MINIMUM_LIQUIDITY - 1);
    let effect = remove_effect(&p, 1, 1, 1);
    let _ = remove_liquidity(&pool_data(&p), &effect);
}

#[test]
#[should_panic(expected = "Pool only contains locked liquidity")]
fn pool_after_remove_rejects_a_pool_at_exactly_the_lock_floor() {
    let p = pool(RESERVE_A, RESERVE_B, MINIMUM_LIQUIDITY);
    let effect = remove_effect(&p, 1, 1, 1);
    let _ = remove_liquidity(&pool_data(&p), &effect);
}

/// The lock is enforced here rather than by reading the lock holding, which this
/// instruction never sees.
#[test]
#[should_panic(expected = "Cannot remove locked minimum liquidity")]
fn pool_after_remove_refuses_to_burn_through_the_permanent_lock() {
    let p = live_pool();
    let effect = remove_effect(&p, LP_SUPPLY - MINIMUM_LIQUIDITY + 1, 1, 1);
    let _ = remove_liquidity(&pool_data(&p), &effect);
}

#[test]
fn pool_after_remove_allows_burning_exactly_the_unlocked_supply() {
    let p = live_pool();
    let effect = remove_effect(&p, LP_SUPPLY - MINIMUM_LIQUIDITY, 1, 1);
    let post = PoolDefinition::try_from(&remove_liquidity(&pool_data(&p), &effect)).unwrap();
    assert_eq!(post.liquidity_pool_supply, MINIMUM_LIQUIDITY);
    // The lock keeps its backing: reserves are never drained to zero.
    assert!(post.reserve_a > 0 && post.reserve_b > 0);
}

#[test]
#[should_panic(expected = "Remove amount must be nonzero")]
fn pool_after_remove_rejects_a_zero_burn() {
    let p = live_pool();
    let mut effect = remove_effect(&p, REMOVE_LP, 1, 1);
    effect.remove_liquidity_amount = 0;
    let _ = remove_liquidity(&pool_data(&p), &effect);
}

#[test]
#[should_panic(expected = "Declared post-removal price does not match")]
fn pool_after_remove_rejects_a_forged_post_price() {
    let p = live_pool();
    let mut effect = remove_effect(&p, REMOVE_LP, 1, 1);
    effect.new_price += 1;
    let _ = remove_liquidity(&pool_data(&p), &effect);
}

#[test]
#[should_panic(expected = "Declared pool identity does not match")]
fn pool_after_remove_rejects_a_forged_identity() {
    let p = live_pool();
    let mut effect = remove_effect(&p, REMOVE_LP, 1, 1);
    effect.identity.liquidity_pool_id = user_lp_id();
    let _ = remove_liquidity(&pool_data(&p), &effect);
}

#[test]
fn pool_after_remove_survives_realistic_eighteen_decimal_amounts() {
    let big = 1_000_000u128 * 10u128.pow(18);
    let p = pool(big, big, big);
    let effect = remove_effect(&p, big / 2, 1, 1);
    let post = PoolDefinition::try_from(&remove_liquidity(&pool_data(&p), &effect)).unwrap();
    assert_eq!(post.liquidity_pool_supply, big / 2);
    assert_eq!(post.reserve_a, big / 2);
}

/// Add then remove the whole unlocked supply: the lock is what stops the pool
/// being emptied, and the reserves it keeps are what back it.
#[test]
fn the_minimum_liquidity_lock_survives_removing_every_user_lp() {
    let (data, _) = plan_definition_ok();
    let created = PoolDefinition::try_from(&data).unwrap();
    let user_lp = created.liquidity_pool_supply - MINIMUM_LIQUIDITY;
    let effect = remove_effect(&created, user_lp, 1, 1);
    let post = PoolDefinition::try_from(&remove_liquidity(&data, &effect)).unwrap();
    assert_eq!(post.liquidity_pool_supply, MINIMUM_LIQUIDITY);
    assert!(post.reserve_a > 0 && post.reserve_b > 0);
}

// ─── Swaps ──────────────────────────────────────────────────────────────────

const SWAP_IN: u128 = 1_000;

/// The post-swap price the pool recomputes, given a binding's amounts.
fn repriced(p: &PoolDefinition, binding: &SwapBinding) -> u128 {
    let (input, output) = p.sides(binding.definition_id_in).unwrap();
    let (reserve_a, reserve_b) = p.reserves_in_ab_order(
        binding.definition_id_in,
        input.reserve + binding.amount_in - binding.protocol_fee,
        output.reserve - binding.amount_out,
    );
    spot_price_q64_64(reserve_a, reserve_b)
}

/// The declaration a caller builds for an exact-input swap: the shared quote,
/// plus the fee split and post-swap price the pool pins.
fn swap_in_binding(
    p: &PoolDefinition,
    definition_id_in: AccountId,
    amount_in: u128,
    min_amount_out: u128,
) -> SwapBinding {
    let (input, output) = p.sides(definition_id_in).unwrap();
    let (effective, amount_out) =
        swap_exact_in_amounts(amount_in, input.reserve, output.reserve, SWAP_FEE_BPS);
    let protocol_fee = protocol_fee_amount(amount_in - effective, PROTOCOL_FEE_BPS);
    let mut binding = SwapBinding {
        identity: identity(),
        definition_id_in,
        amount_in,
        amount_out,
        protocol_fee,
        new_price: 0,
        swap_fee_bps: SWAP_FEE_BPS,
        protocol_fee_bps: PROTOCOL_FEE_BPS,
        limit: min_amount_out,
    };
    binding.new_price = repriced(p, &binding);
    binding
}

/// The mirror for an exact-output swap: `limit` is the ceiling on the input.
fn swap_out_binding(
    p: &PoolDefinition,
    definition_id_in: AccountId,
    amount_out: u128,
    max_amount_in: u128,
) -> SwapBinding {
    let (input, output) = p.sides(definition_id_in).unwrap();
    let (effective, amount_in) =
        swap_exact_out_amounts(amount_out, input.reserve, output.reserve, SWAP_FEE_BPS).unwrap();
    let protocol_fee = protocol_fee_amount(amount_in - effective, PROTOCOL_FEE_BPS);
    let mut binding = SwapBinding {
        identity: identity(),
        definition_id_in,
        amount_in,
        amount_out,
        protocol_fee,
        new_price: 0,
        swap_fee_bps: SWAP_FEE_BPS,
        protocol_fee_bps: PROTOCOL_FEE_BPS,
        limit: max_amount_in,
    };
    binding.new_price = repriced(p, &binding);
    binding
}

struct SwapSlots {
    pool: AccountMeta,
    vault_a: AccountMeta,
    vault_b: AccountMeta,
    user_in: AccountMeta,
    user_out: AccountMeta,
    current_tick: AccountMeta,
    protocol_fee: AccountMeta,
}

impl SwapSlots {
    /// The slots for a swap entering via `definition_id_in`.
    fn for_input(definition_id_in: AccountId) -> Self {
        Self {
            pool: ours(pool_id()),
            vault_a: token_slot(vault_a_id()),
            vault_b: token_slot(vault_b_id()),
            user_in: signed_token_slot(user_a_id()),
            user_out: token_slot(user_b_id()),
            current_tick: oracle_slot(current_tick_id()),
            protocol_fee: token_slot(protocol_fee_id(definition_id_in)),
        }
    }
}

fn plan_swap_with(slots: &SwapSlots, binding: &SwapBinding) -> Vec<lee_core::program::ChainedCall> {
    plan_swap(
        &ours(config_id()),
        &slots.pool,
        &slots.vault_a,
        &slots.vault_b,
        &slots.user_in,
        &slots.user_out,
        &slots.current_tick,
        &clock_slot(),
        &slots.protocol_fee,
        &facts(),
        binding,
        NOW,
        AMM_PROGRAM_ID,
    )
}

// ── plan half ──

/// Input transfer, output transfer, protocol fee, tick refresh.
#[test]
fn plan_swap_chains_four_calls_when_a_protocol_fee_is_taken() {
    let binding = swap_in_binding(&live_pool(), definition_a(), SWAP_IN, 1);
    assert!(binding.protocol_fee > 0, "fixture must take a protocol fee");
    let calls = plan_swap_with(&SwapSlots::for_input(definition_a()), &binding);
    assert_eq!(calls.len(), 4);

    assert_eq!(
        transfer_parts(&calls[0]),
        (user_a_id(), vault_a_id(), definition_a(), SWAP_IN),
        "the trader pays the full input, protocol fee included"
    );
    assert!(
        calls[0].pda_seeds.is_empty(),
        "debiting the trader is authorized by their own signature, not a seed"
    );

    assert_eq!(
        transfer_parts(&calls[1]),
        (
            vault_b_id(),
            user_b_id(),
            definition_b(),
            binding.amount_out
        )
    );
    assert_eq!(
        calls[1].pda_seeds,
        vec![amm_core::compute_vault_pda_seed(pool_id(), definition_b())]
    );

    assert_eq!(
        transfer_parts(&calls[2]),
        (
            vault_a_id(),
            protocol_fee_id(definition_a()),
            definition_a(),
            binding.protocol_fee
        ),
        "the protocol's cut is taken in the input token"
    );
    assert_eq!(
        calls[2].pda_seeds,
        vec![
            amm_core::compute_vault_pda_seed(pool_id(), definition_a()),
            amm_core::compute_protocol_fee_pda_seed(config_id(), definition_a()),
        ],
        "the vault's seed debits, the protocol holding's seed creates it on first use"
    );

    match oracle_instruction(&calls[3]) {
        twap_oracle_core::Instruction::UpdateCurrentTick { price, now } => {
            assert_eq!(price, binding.new_price);
            assert_eq!(now, NOW);
        }
        _ => panic!("expected UpdateCurrentTick"),
    }
}

#[test]
fn plan_swap_omits_the_protocol_transfer_when_the_fee_rounds_to_zero() {
    let mut binding = swap_in_binding(&live_pool(), definition_a(), 100, 1);
    binding.protocol_fee = 0;
    binding.new_price = repriced(&live_pool(), &binding);
    let calls = plan_swap_with(&SwapSlots::for_input(definition_a()), &binding);
    assert_eq!(calls.len(), 3);
}

/// Direction is declared, so the plan resolves the vaults from the declaration
/// rather than from the trader's holding.
#[test]
fn plan_swap_resolves_the_vaults_from_the_declared_direction() {
    let binding = swap_in_binding(&live_pool(), definition_b(), SWAP_IN, 1);
    let mut slots = SwapSlots::for_input(definition_b());
    slots.user_in = signed_token_slot(user_b_id());
    slots.user_out = token_slot(user_a_id());
    let calls = plan_swap_with(&slots, &binding);
    assert_eq!(
        transfer_parts(&calls[0]),
        (user_b_id(), vault_b_id(), definition_b(), SWAP_IN),
        "entering via token B debits vault B"
    );
    assert_eq!(
        transfer_parts(&calls[1]).0,
        vault_a_id(),
        "and pays out of vault A"
    );
}

#[test]
#[should_panic(expected = "AccountId is not a token type for the pool")]
fn plan_swap_rejects_a_direction_outside_the_pair() {
    let mut binding = swap_in_binding(&live_pool(), definition_a(), SWAP_IN, 1);
    binding.definition_id_in = definition_c();
    let _ = plan_swap_with(&SwapSlots::for_input(definition_c()), &binding);
}

#[test]
#[should_panic(expected = "Vault A was not provided")]
fn plan_swap_rejects_a_wrong_vault_a_slot() {
    let binding = swap_in_binding(&live_pool(), definition_a(), SWAP_IN, 1);
    let mut slots = SwapSlots::for_input(definition_a());
    slots.vault_a = token_slot(AccountId::new([0xAAu8; 32]));
    let _ = plan_swap_with(&slots, &binding);
}

#[test]
#[should_panic(expected = "Vault B was not provided")]
fn plan_swap_rejects_a_wrong_vault_b_slot() {
    let binding = swap_in_binding(&live_pool(), definition_a(), SWAP_IN, 1);
    let mut slots = SwapSlots::for_input(definition_a());
    slots.vault_b = token_slot(AccountId::new([0xAAu8; 32]));
    let _ = plan_swap_with(&slots, &binding);
}

#[test]
#[should_panic(expected = "not derived under this config's namespace")]
fn plan_swap_rejects_a_pool_outside_the_config_namespace() {
    let binding = swap_in_binding(&live_pool(), definition_a(), SWAP_IN, 1);
    let mut slots = SwapSlots::for_input(definition_a());
    slots.pool = ours(AccountId::new([0xAAu8; 32]));
    let _ = plan_swap_with(&slots, &binding);
}

#[test]
#[should_panic(expected = "must authorize the transfer that debits it")]
fn plan_swap_rejects_an_unsigned_input_holding() {
    let binding = swap_in_binding(&live_pool(), definition_a(), SWAP_IN, 1);
    let mut slots = SwapSlots::for_input(definition_a());
    slots.user_in = token_slot(user_a_id());
    let _ = plan_swap_with(&slots, &binding);
}

/// A trader holding that is also a vault would turn a self-transfer into free
/// funding, or a payout the reserves never backed.
#[test]
#[should_panic(expected = "a trader holding cannot be a pool vault")]
fn plan_swap_rejects_an_input_holding_that_is_a_vault() {
    let binding = swap_in_binding(&live_pool(), definition_a(), SWAP_IN, 1);
    let mut slots = SwapSlots::for_input(definition_a());
    slots.user_in = signed_token_slot(vault_a_id());
    let _ = plan_swap_with(&slots, &binding);
}

#[test]
#[should_panic(expected = "a trader holding cannot be a pool vault")]
fn plan_swap_rejects_an_output_holding_that_is_a_vault() {
    let binding = swap_in_binding(&live_pool(), definition_a(), SWAP_IN, 1);
    let mut slots = SwapSlots::for_input(definition_a());
    slots.user_out = token_slot(vault_b_id());
    let _ = plan_swap_with(&slots, &binding);
}

#[test]
#[should_panic(expected = "protocol-fee holding does not match the input token's protocol PDA")]
fn plan_swap_rejects_a_protocol_holding_for_the_wrong_token() {
    let binding = swap_in_binding(&live_pool(), definition_a(), SWAP_IN, 1);
    let mut slots = SwapSlots::for_input(definition_a());
    slots.protocol_fee = token_slot(protocol_fee_id(definition_b()));
    let _ = plan_swap_with(&slots, &binding);
}

#[test]
#[should_panic(expected = "current tick Account ID does not match PDA")]
fn plan_swap_rejects_a_wrong_current_tick_slot() {
    let binding = swap_in_binding(&live_pool(), definition_a(), SWAP_IN, 1);
    let mut slots = SwapSlots::for_input(definition_a());
    slots.current_tick = oracle_slot(AccountId::new([0xAAu8; 32]));
    let _ = plan_swap_with(&slots, &binding);
}

// ── apply half, exact input ──

#[test]
fn pool_after_swap_in_records_the_trade() {
    let p = live_pool();
    let binding = swap_in_binding(&p, definition_a(), SWAP_IN, 1);
    let post = PoolDefinition::try_from(&swap_exact_in(&pool_data(&p), &binding)).unwrap();
    assert_eq!(
        post.reserve_a,
        RESERVE_A + SWAP_IN - binding.protocol_fee,
        "reserves grow by the input net of the fee that left the vault"
    );
    assert_eq!(post.reserve_b, RESERVE_B - binding.amount_out);
    assert_eq!(post.liquidity_pool_supply, LP_SUPPLY, "a swap mints no LP");
}

/// The constant product grows: the providers' share of the swap fee accrues as
/// invariant growth rather than as a separate balance.
#[test]
fn pool_after_swap_in_grows_the_invariant() {
    let p = live_pool();
    let binding = swap_in_binding(&p, definition_a(), SWAP_IN, 1);
    let post = PoolDefinition::try_from(&swap_exact_in(&pool_data(&p), &binding)).unwrap();
    assert!(post.reserve_a * post.reserve_b > RESERVE_A * RESERVE_B);
}

#[test]
fn pool_after_swap_in_works_in_both_directions() {
    let p = live_pool();
    let binding = swap_in_binding(&p, definition_b(), SWAP_IN, 1);
    let post = PoolDefinition::try_from(&swap_exact_in(&pool_data(&p), &binding)).unwrap();
    assert_eq!(post.reserve_b, RESERVE_B + SWAP_IN - binding.protocol_fee);
    assert_eq!(post.reserve_a, RESERVE_A - binding.amount_out);
}

/// The bound, not an equality: an under-claimed output leaves the surplus in the
/// pool, and that is what makes a stale quote safe rather than fatal.
#[test]
fn pool_after_swap_in_accepts_less_output_than_the_live_quote() {
    let p = live_pool();
    let mut binding = swap_in_binding(&p, definition_a(), SWAP_IN, 1);
    binding.amount_out -= 10;
    binding.new_price = repriced(&p, &binding);
    let post = PoolDefinition::try_from(&swap_exact_in(&pool_data(&p), &binding)).unwrap();
    assert_eq!(post.reserve_b, RESERVE_B - binding.amount_out);
}

#[test]
#[should_panic(expected = "cannot afford this offer at its live price")]
fn pool_after_swap_in_rejects_more_output_than_the_live_quote() {
    let p = live_pool();
    let mut binding = swap_in_binding(&p, definition_a(), SWAP_IN, 1);
    binding.amount_out += 1;
    binding.new_price = repriced(&p, &binding);
    let _ = swap_exact_in(&pool_data(&p), &binding);
}

/// The whole reason the bound is evaluated here: a quote taken against larger
/// reserves than the pool now holds is refused rather than paid out.
#[test]
#[should_panic(expected = "cannot afford this offer at its live price")]
fn pool_after_swap_in_rejects_an_offer_quoted_against_richer_reserves() {
    let quoted_against = pool(RESERVE_A, RESERVE_B * 2, LP_SUPPLY);
    let binding = swap_in_binding(&quoted_against, definition_a(), SWAP_IN, 1);
    let _ = swap_exact_in(&pool_data(&live_pool()), &binding);
}

#[test]
#[should_panic(expected = "less than minimal amount out")]
fn pool_after_swap_in_rejects_output_below_the_callers_floor() {
    let p = live_pool();
    let mut binding = swap_in_binding(&p, definition_a(), SWAP_IN, 1);
    binding.limit = binding.amount_out + 1;
    let _ = swap_exact_in(&pool_data(&p), &binding);
}

#[test]
#[should_panic(expected = "Declared protocol fee does not match")]
fn pool_after_swap_in_rejects_an_understated_protocol_fee() {
    let p = live_pool();
    let mut binding = swap_in_binding(&p, definition_a(), SWAP_IN, 1);
    binding.protocol_fee -= 1;
    binding.new_price = repriced(&p, &binding);
    let _ = swap_exact_in(&pool_data(&p), &binding);
}

#[test]
#[should_panic(expected = "Declared protocol fee does not match")]
fn pool_after_swap_in_rejects_an_overstated_protocol_fee() {
    let p = live_pool();
    let mut binding = swap_in_binding(&p, definition_a(), SWAP_IN, 1);
    binding.protocol_fee += 1;
    binding.new_price = repriced(&p, &binding);
    let _ = swap_exact_in(&pool_data(&p), &binding);
}

/// The fee tiers travel in the binding, but they are copied from the declared
/// `ConfigFacts` — so a cheaper fee here is a fact the config guard refuses, and
/// on its own it does not even price a better trade.
#[test]
#[should_panic(expected = "cannot afford this offer at its live price")]
fn pool_after_swap_in_prices_with_the_fee_the_binding_carries() {
    let p = live_pool();
    let mut binding = swap_in_binding(&p, definition_a(), SWAP_IN, 1);
    // The output was quoted at a 0-bps fee; the binding still declares 30.
    let zero_fee = SwapBinding {
        swap_fee_bps: 0,
        ..binding
    };
    let (_, bigger_out) =
        swap_exact_in_amounts(SWAP_IN, RESERVE_A, RESERVE_B, zero_fee.swap_fee_bps);
    binding.amount_out = bigger_out;
    binding.new_price = repriced(&p, &binding);
    let _ = swap_exact_in(&pool_data(&p), &binding);
}

#[test]
#[should_panic(expected = "Swap amounts must be nonzero")]
fn pool_after_swap_in_rejects_a_zero_output() {
    let p = live_pool();
    let mut binding = swap_in_binding(&p, definition_a(), SWAP_IN, 0);
    binding.amount_out = 0;
    let _ = swap_exact_in(&pool_data(&p), &binding);
}

#[test]
#[should_panic(expected = "Effective swap amount should be nonzero")]
fn pool_after_swap_in_rejects_an_input_that_rounds_away_entirely() {
    // 1 unit at 30 bps: `1 * 9970 / 10000` floors to zero effective input.
    let p = live_pool();
    let mut binding = swap_in_binding(&p, definition_a(), 1, 0);
    binding.amount_out = 1;
    binding.protocol_fee = 0;
    binding.new_price = repriced(&p, &binding);
    let _ = swap_exact_in(&pool_data(&p), &binding);
}

#[test]
#[should_panic(expected = "Swap output exhausts the reserve")]
fn pool_after_swap_in_rejects_draining_the_output_reserve() {
    let p = live_pool();
    let mut binding = swap_in_binding(&p, definition_a(), SWAP_IN, 1);
    binding.amount_out = RESERVE_B;
    let _ = swap_exact_in(&pool_data(&p), &binding);
}

#[test]
#[should_panic(expected = "Pool reserves must be nonzero")]
fn pool_after_swap_in_rejects_a_pool_with_a_zero_reserve() {
    let binding = swap_in_binding(&live_pool(), definition_a(), SWAP_IN, 1);
    let _ = swap_exact_in(&pool_data(&pool(0, RESERVE_B, LP_SUPPLY)), &binding);
}

#[test]
#[should_panic(expected = "below minimum liquidity")]
fn pool_after_swap_in_rejects_a_pool_below_the_lock_floor() {
    let p = pool(RESERVE_A, RESERVE_B, MINIMUM_LIQUIDITY - 1);
    let binding = swap_in_binding(&p, definition_a(), SWAP_IN, 1);
    let _ = swap_exact_in(&pool_data(&p), &binding);
}

#[test]
#[should_panic(expected = "Declared post-swap price does not match")]
fn pool_after_swap_in_rejects_a_forged_post_price() {
    let p = live_pool();
    let mut binding = swap_in_binding(&p, definition_a(), SWAP_IN, 1);
    binding.new_price += 1;
    let _ = swap_exact_in(&pool_data(&p), &binding);
}

#[test]
#[should_panic(expected = "Declared pool identity does not match")]
fn pool_after_swap_in_rejects_a_forged_identity() {
    let p = live_pool();
    let mut binding = swap_in_binding(&p, definition_a(), SWAP_IN, 1);
    binding.identity.vault_b_id = user_b_id();
    let _ = swap_exact_in(&pool_data(&p), &binding);
}

#[test]
fn pool_after_swap_in_survives_realistic_eighteen_decimal_amounts() {
    let big = 1_000_000u128 * 10u128.pow(18);
    let p = pool(big, big, big);
    let binding = swap_in_binding(&p, definition_a(), big / 1_000, 1);
    let post = PoolDefinition::try_from(&swap_exact_in(&pool_data(&p), &binding)).unwrap();
    assert!(post.reserve_a > big && post.reserve_b < big);
}

// ── apply half, exact output ──

#[test]
fn pool_after_swap_out_records_the_trade() {
    let p = live_pool();
    let binding = swap_out_binding(&p, definition_a(), 1_662, u128::MAX);
    let post = PoolDefinition::try_from(&swap_exact_out(&pool_data(&p), &binding)).unwrap();
    assert_eq!(post.reserve_b, RESERVE_B - 1_662, "the output is exact");
    assert_eq!(
        post.reserve_a,
        RESERVE_A + binding.amount_in - binding.protocol_fee
    );
}

#[test]
fn pool_after_swap_out_works_in_both_directions() {
    let p = live_pool();
    let binding = swap_out_binding(&p, definition_b(), 400, u128::MAX);
    let post = PoolDefinition::try_from(&swap_exact_out(&pool_data(&p), &binding)).unwrap();
    assert_eq!(post.reserve_a, RESERVE_A - 400);
}

/// The mirror bound: paying *more* than the curve demands is safe for the pool.
#[test]
fn pool_after_swap_out_accepts_more_input_than_the_live_quote() {
    let p = live_pool();
    let mut binding = swap_out_binding(&p, definition_a(), 1_662, u128::MAX);
    binding.amount_in += 50;
    binding.protocol_fee = protocol_fee_amount(
        binding.amount_in
            - swap_exact_out_amounts(binding.amount_out, RESERVE_A, RESERVE_B, SWAP_FEE_BPS)
                .unwrap()
                .0,
        PROTOCOL_FEE_BPS,
    );
    binding.new_price = repriced(&p, &binding);
    let post = PoolDefinition::try_from(&swap_exact_out(&pool_data(&p), &binding)).unwrap();
    assert_eq!(
        post.reserve_a,
        RESERVE_A + binding.amount_in - binding.protocol_fee
    );
}

#[test]
#[should_panic(expected = "below what the pool's live price demands")]
fn pool_after_swap_out_rejects_less_input_than_the_live_quote() {
    let p = live_pool();
    let mut binding = swap_out_binding(&p, definition_a(), 1_662, u128::MAX);
    binding.amount_in -= 1;
    binding.new_price = repriced(&p, &binding);
    let _ = swap_exact_out(&pool_data(&p), &binding);
}

#[test]
#[should_panic(expected = "below what the pool's live price demands")]
fn pool_after_swap_out_rejects_a_quote_taken_against_richer_reserves() {
    let quoted_against = pool(RESERVE_A, RESERVE_B * 2, LP_SUPPLY);
    let binding = swap_out_binding(&quoted_against, definition_a(), 1_662, u128::MAX);
    let _ = swap_exact_out(&pool_data(&live_pool()), &binding);
}

#[test]
#[should_panic(expected = "Required input exceeds maximum amount in")]
fn pool_after_swap_out_rejects_input_above_the_callers_ceiling() {
    let p = live_pool();
    let binding = swap_out_binding(&p, definition_a(), 1_662, 100);
    let _ = swap_exact_out(&pool_data(&p), &binding);
}

#[test]
#[should_panic(expected = "Swap output exhausts the reserve")]
fn pool_after_swap_out_rejects_draining_the_output_reserve() {
    let p = live_pool();
    let mut binding = swap_out_binding(&p, definition_a(), 1_662, u128::MAX);
    binding.amount_out = RESERVE_B;
    let _ = swap_exact_out(&pool_data(&p), &binding);
}

#[test]
#[should_panic(expected = "Swap amounts must be nonzero")]
fn pool_after_swap_out_rejects_a_zero_output() {
    let p = live_pool();
    let mut binding = swap_out_binding(&p, definition_a(), 1_662, u128::MAX);
    binding.amount_out = 0;
    let _ = swap_exact_out(&pool_data(&p), &binding);
}

#[test]
#[should_panic(expected = "Declared protocol fee does not match")]
fn pool_after_swap_out_rejects_a_forged_protocol_fee() {
    let p = live_pool();
    let mut binding = swap_out_binding(&p, definition_a(), 1_662, u128::MAX);
    binding.protocol_fee += 1;
    binding.new_price = repriced(&p, &binding);
    let _ = swap_exact_out(&pool_data(&p), &binding);
}

#[test]
#[should_panic(expected = "Declared post-swap price does not match")]
fn pool_after_swap_out_rejects_a_forged_post_price() {
    let p = live_pool();
    let mut binding = swap_out_binding(&p, definition_a(), 1_662, u128::MAX);
    binding.new_price += 1;
    let _ = swap_exact_out(&pool_data(&p), &binding);
}

#[test]
fn pool_after_swap_out_survives_realistic_eighteen_decimal_amounts() {
    let big = 1_000_000u128 * 10u128.pow(18);
    let p = pool(big, big, big);
    let binding = swap_out_binding(&p, definition_a(), big / 1_000, u128::MAX);
    let post = PoolDefinition::try_from(&swap_exact_out(&pool_data(&p), &binding)).unwrap();
    assert_eq!(post.reserve_b, big - big / 1_000);
}

/// The two modes agree: an exact-output swap for what an exact-input swap would
/// have yielded demands at most the same input back.
#[test]
fn the_two_swap_modes_are_consistent() {
    let p = live_pool();
    let forward = swap_in_binding(&p, definition_a(), SWAP_IN, 1);
    let backward = swap_out_binding(&p, definition_a(), forward.amount_out, u128::MAX);
    assert!(
        backward.amount_in <= forward.amount_in,
        "exact-output rounding never asks for more than the exact-input leg paid"
    );
}

// ─── SyncReserves ───────────────────────────────────────────────────────────

fn sync_effect(vault_a_balance: u128, vault_b_balance: u128) -> SyncReserves {
    SyncReserves {
        identity: identity(),
        vault_a_balance,
        vault_b_balance,
    }
}

fn plan_sync(
    pool_slot: &AccountMeta,
    effect: &SyncReserves,
) -> (
    TokenDescriptor,
    TokenDescriptor,
    Vec<lee_core::program::ChainedCall>,
) {
    plan_sync_reserves(
        &ours(config_id()),
        pool_slot,
        &token_slot(vault_a_id()),
        &token_slot(vault_b_id()),
        &oracle_slot(current_tick_id()),
        &clock_slot(),
        &facts(),
        effect,
        NOW,
        AMM_PROGRAM_ID,
    )
}

#[test]
fn plan_sync_reserves_derives_the_tick_from_the_pinned_balances() {
    let effect = sync_effect(RESERVE_A + 500, RESERVE_B);
    let (descriptor_a, descriptor_b, calls) = plan_sync(&ours(pool_id()), &effect);
    assert_eq!(descriptor_a, descriptor(definition_a()));
    assert_eq!(descriptor_b, descriptor(definition_b()));
    assert_eq!(calls.len(), 1);
    match oracle_instruction(&calls[0]) {
        twap_oracle_core::Instruction::UpdateCurrentTick { price, now } => {
            assert_eq!(price, spot_price_q64_64(RESERVE_A + 500, RESERVE_B));
            assert_eq!(now, NOW);
        }
        _ => panic!("expected UpdateCurrentTick"),
    }
    assert_eq!(
        calls[0].pda_seeds,
        vec![amm_core::compute_pool_pda_seed(
            config_id(),
            definition_a(),
            definition_b()
        )]
    );
}

#[test]
#[should_panic(expected = "not derived under this config's namespace")]
fn plan_sync_reserves_rejects_a_pool_outside_the_config_namespace() {
    plan_sync(
        &ours(AccountId::new([0xAAu8; 32])),
        &sync_effect(RESERVE_A, RESERVE_B),
    );
}

/// A donation straight into a vault leaves the pool mispriced; syncing is how
/// the reserves catch up.
#[test]
fn pool_after_sync_absorbs_a_donation() {
    let p = live_pool();
    let effect = sync_effect(RESERVE_A + 1_000, RESERVE_B);
    let post = PoolDefinition::try_from(&sync_reserves(&pool_data(&p), &effect)).unwrap();
    assert_eq!(post.reserve_a, RESERVE_A + 1_000);
    assert_eq!(post.reserve_b, RESERVE_B);
    assert_eq!(
        post.liquidity_pool_supply, LP_SUPPLY,
        "a donation mints no LP: it accrues to the existing providers"
    );
}

/// Reserves may lag a vault but never exceed it, so the other direction is a
/// broken invariant to report rather than a state to normalize.
#[test]
#[should_panic(expected = "vault A balance is less than its reserve")]
fn pool_after_sync_rejects_an_under_collateralized_vault_a() {
    let p = live_pool();
    let _ = sync_reserves(&pool_data(&p), &sync_effect(RESERVE_A - 1, RESERVE_B));
}

#[test]
#[should_panic(expected = "vault B balance is less than its reserve")]
fn pool_after_sync_rejects_an_under_collateralized_vault_b() {
    let p = live_pool();
    let _ = sync_reserves(&pool_data(&p), &sync_effect(RESERVE_A, RESERVE_B - 1));
}

#[test]
#[should_panic(expected = "below minimum liquidity")]
fn pool_after_sync_rejects_a_pool_below_the_lock_floor() {
    let p = pool(RESERVE_A, RESERVE_B, MINIMUM_LIQUIDITY - 1);
    let _ = sync_reserves(&pool_data(&p), &sync_effect(RESERVE_A, RESERVE_B));
}

#[test]
#[should_panic(expected = "Declared pool identity does not match")]
fn pool_after_sync_rejects_a_forged_identity() {
    let p = live_pool();
    let mut effect = sync_effect(RESERVE_A, RESERVE_B);
    effect.identity.definition_token_a_id = definition_c();
    let _ = sync_reserves(&pool_data(&p), &effect);
}

/// The donation → mispricing → sync sequence end to end: an add against stale
/// reserves prices the deposit off the pool's recorded ratio, and a sync first
/// is what makes the deposit fair.
#[test]
fn syncing_before_an_add_prices_the_deposit_against_the_real_vaults() {
    let p = live_pool();
    let donated_a = RESERVE_A + 1_000;

    // Unsynced: 500 of A still buys the full 707 LP, because the pool's recorded
    // reserve_a has not caught up with the donation.
    let (_, _, stale_lp) = quote_add_liquidity(&p, MAX_ADD_A, MAX_ADD_B);
    assert_eq!(stale_lp, 707);

    // Synced: the same deposit buys proportionally less, which is the donation
    // accruing to the providers who were there for it.
    let synced = PoolDefinition::try_from(&sync_reserves(
        &pool_data(&p),
        &sync_effect(donated_a, RESERVE_B),
    ))
    .unwrap();
    let (_, _, fair_lp) = quote_add_liquidity(&synced, MAX_ADD_A, MAX_ADD_B);
    assert!(
        fair_lp < stale_lp,
        "syncing must make the stale deposit buy less, not more"
    );
}

// ─── the vault guard ────────────────────────────────────────────────────────

#[test]
fn check_vault_balance_accepts_the_stored_balance() {
    check_vault_balance(
        &holding(definition_a(), 5_000),
        &CheckVaultBalance {
            descriptor: descriptor(definition_a()),
            balance: 5_000,
        },
    );
}

#[test]
#[should_panic(expected = "Declared vault balance does not match")]
fn check_vault_balance_rejects_a_forged_balance() {
    check_vault_balance(
        &holding(definition_a(), 5_000),
        &CheckVaultBalance {
            descriptor: descriptor(definition_a()),
            balance: 6_000,
        },
    );
}

#[test]
#[should_panic(expected = "Vault holds a different token")]
fn check_vault_balance_rejects_a_vault_of_another_token() {
    check_vault_balance(
        &holding(definition_c(), 5_000),
        &CheckVaultBalance {
            descriptor: descriptor(definition_a()),
            balance: 5_000,
        },
    );
}

#[test]
#[should_panic(expected = "valid Token Holding Account")]
fn check_vault_balance_rejects_an_empty_vault_shard() {
    check_vault_balance(
        &empty(),
        &CheckVaultBalance {
            descriptor: descriptor(definition_a()),
            balance: 0,
        },
    );
}

#[test]
#[should_panic(expected = "valid Fungible Token Holding Account")]
fn check_vault_balance_rejects_a_non_fungible_vault() {
    let nft = ShardData::from(&TokenHolding::NftPrintedCopy {
        definition_id: definition_a(),
        owned: true,
    });
    check_vault_balance(
        &nft,
        &CheckVaultBalance {
            descriptor: descriptor(definition_a()),
            balance: 0,
        },
    );
}

// ─── WithdrawProtocolFees ───────────────────────────────────────────────────

fn plan_withdraw(
    protocol_fee_holding: &AccountMeta,
    authority: &AccountMeta,
    amount: u128,
    descriptor: &TokenDescriptor,
) -> lee_core::program::ChainedCall {
    plan_withdraw_protocol_fees(
        &ours(config_id()),
        protocol_fee_holding,
        &token_slot(destination_id()),
        authority,
        amount,
        &facts(),
        descriptor,
        AMM_PROGRAM_ID,
    )
}

#[test]
fn plan_withdraw_protocol_fees_moves_the_declared_token_under_the_pda_seed() {
    let call = plan_withdraw(
        &token_slot(protocol_fee_id(definition_a())),
        &AccountMeta::new(authority_id(), true, AMM_PROGRAM_ID),
        250,
        &descriptor(definition_a()),
    );
    assert_eq!(call.program_account_id, TOKEN_PROGRAM_ID);
    assert_eq!(
        transfer_parts(&call),
        (
            protocol_fee_id(definition_a()),
            destination_id(),
            definition_a(),
            250
        )
    );
    assert_eq!(
        call.pda_seeds,
        vec![amm_core::compute_protocol_fee_pda_seed(
            config_id(),
            definition_a()
        )]
    );
}

/// The descriptor is what makes both holdings vouch for the asset: the Token
/// Program asserts it on the debit *and* the credit, which is what the two
/// holding reads used to establish.
#[test]
fn plan_withdraw_protocol_fees_pins_both_holdings_through_one_descriptor() {
    let call = plan_withdraw(
        &token_slot(protocol_fee_id(definition_b())),
        &AccountMeta::new(authority_id(), true, AMM_PROGRAM_ID),
        1,
        &descriptor(definition_b()),
    );
    match token_instruction(&call) {
        token_core::Instruction::Transfer { descriptor, .. } => {
            assert_eq!(descriptor.definition_id, definition_b());
            assert_eq!(descriptor.kind, TokenKind::Fungible);
        }
        _ => panic!("expected a Transfer"),
    }
    assert_eq!(
        selector_ids(&call),
        vec![protocol_fee_id(definition_b()), destination_id()]
    );
}

#[test]
#[should_panic(expected = "not the configured admin authority")]
fn plan_withdraw_protocol_fees_rejects_a_caller_who_is_not_the_admin() {
    let _ = plan_withdraw(
        &token_slot(protocol_fee_id(definition_a())),
        &AccountMeta::new(other_authority_id(), true, AMM_PROGRAM_ID),
        1,
        &descriptor(definition_a()),
    );
}

#[test]
#[should_panic(expected = "must authorize the withdrawal")]
fn plan_withdraw_protocol_fees_rejects_an_unauthorized_admin() {
    let _ = plan_withdraw(
        &token_slot(protocol_fee_id(definition_a())),
        &AccountMeta::new(authority_id(), false, AMM_PROGRAM_ID),
        1,
        &descriptor(definition_a()),
    );
}

#[test]
#[should_panic(expected = "does not match the protocol-fee PDA")]
fn plan_withdraw_protocol_fees_rejects_an_arbitrary_source_holding() {
    let _ = plan_withdraw(
        &token_slot(vault_a_id()),
        &AccountMeta::new(authority_id(), true, AMM_PROGRAM_ID),
        1,
        &descriptor(definition_a()),
    );
}

/// The source address and the declared token must agree: the PDA is derived from
/// the descriptor, so naming token A's holding while declaring token B fails.
#[test]
#[should_panic(expected = "does not match the protocol-fee PDA")]
fn plan_withdraw_protocol_fees_rejects_a_holding_for_a_different_declared_token() {
    let _ = plan_withdraw(
        &token_slot(protocol_fee_id(definition_a())),
        &AccountMeta::new(authority_id(), true, AMM_PROGRAM_ID),
        1,
        &descriptor(definition_b()),
    );
}

/// Each `(config, token)` pair has its own holding, so one call drains one token
/// of one instance.
#[test]
fn protocol_fee_pdas_are_distinct_per_token_and_instance() {
    let other_config = compute_config_pda(AMM_PROGRAM_ID, owner_id(), [7u8; 32]);
    assert_ne!(
        protocol_fee_id(definition_a()),
        protocol_fee_id(definition_b())
    );
    assert_ne!(
        protocol_fee_id(definition_a()),
        compute_protocol_fee_pda(AMM_PROGRAM_ID, other_config, definition_a()),
    );
}

// ─── fee arithmetic ─────────────────────────────────────────────────────────

#[test]
fn the_protocol_cut_is_a_fraction_of_the_swap_fee_not_of_the_input() {
    let p = live_pool();
    let binding = swap_in_binding(&p, definition_a(), SWAP_IN, 1);
    let (effective, _) = swap_exact_in_amounts(SWAP_IN, RESERVE_A, RESERVE_B, SWAP_FEE_BPS);
    let swap_fee = SWAP_IN - effective;
    assert_eq!(
        binding.protocol_fee,
        mul_div_floor(swap_fee, PROTOCOL_FEE_BPS, FEE_BPS_DENOMINATOR)
    );
    assert!(
        binding.protocol_fee < swap_fee,
        "the rest of the fee stays with the providers"
    );
}

#[test]
fn a_zero_protocol_fee_tier_leaves_the_whole_swap_fee_with_the_providers() {
    assert_eq!(protocol_fee_amount(1_000, 0), 0);
}

#[test]
fn a_full_protocol_fee_tier_takes_the_whole_swap_fee() {
    assert_eq!(
        protocol_fee_amount(1_000, FEE_BPS_DENOMINATOR),
        1_000,
        "100% of the swap fee, not 100% of the input"
    );
}
