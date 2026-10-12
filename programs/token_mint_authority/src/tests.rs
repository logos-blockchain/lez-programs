//! Unit tests for the `faucet_mint` two-phase implementation.
//!
//! `rejects_foreign_owned_allowance` has no successor: it asserted
//! `mint_allowance.program_owner == token_mint_authority_program_id`. A shard is
//! keyed by the program that wrote it, and emitting an effect on one asserts it
//! is ours — the check lives in SPEL now and is tested there.
//!
//! `rejects_creating_a_holding_the_caller_cannot_sign_for` is also gone, but for
//! a different reason: it is a *deliberate* semantic change. The rule existed
//! only because v0.2.5's `acquire_ownership_on_data_write` let a chained mint
//! claim any unowned address permanently. Writing `shards[token]` denies that
//! address to nobody, so there is nothing left to squat on and the restriction
//! bought only inconvenience.

#![allow(
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    clippy::wildcard_enum_match_arm,
    reason = "tests deliberately panic on bad state via assert!/#[should_panic], index fixed-size vectors, and match one chained-call variant against a borsh-only instruction enum"
)]

use lee_core::account::AccountId;
use token_core::Instruction as TokenInstruction;
use token_mint_authority_core::{MintAllowance, FAUCET_MINT_AMOUNT, MINT_COOLDOWN_MS};

use crate::{
    faucet_mint::{check_clock, check_faucet_authority, plan_faucet_mint, record_mint},
    test_support::{
        allowance_data, allowance_slot, allowance_slot_at, authority_slot, authority_slot_at,
        clock_data, clock_slot, clock_slot_at, definition_id, definition_slot, empty,
        faucet_definition, holding_data, mint_authority_id, non_fungible_definition,
        recipient_account, recipient_id, user_holding_slot, NOW, TOKEN_MINT_AUTHORITY_PROGRAM_ID,
        TOKEN_PROGRAM_ID,
    },
};

fn plan() -> lee_core::program::ChainedCall {
    plan_faucet_mint(
        &recipient_account(true),
        &allowance_slot(),
        &user_holding_slot(),
        &definition_slot(),
        &authority_slot(),
        &clock_slot(),
        TOKEN_MINT_AUTHORITY_PROGRAM_ID,
    )
}

// ── plan half ───────────────────────────────────────────────────────────────

#[test]
fn chained_call_delegates_fixed_mint_to_token_program() {
    let call = plan();
    assert_eq!(call.program_account_id, TOKEN_PROGRAM_ID);
    // `token_core::Instruction` derives only Borsh, so match rather than compare.
    match borsh::from_slice::<TokenInstruction>(&call.instruction_data)
        .expect("chained instruction must decode as a token_core::Instruction")
    {
        TokenInstruction::MintWithAuthority { amount_to_mint } => {
            assert_eq!(amount_to_mint, FAUCET_MINT_AMOUNT, "the grant is fixed");
        }
        _ => panic!("the faucet must delegate MintWithAuthority"),
    }
}

/// `[definition, holding, authority]`, each naming the Token Program's shard.
#[test]
fn chained_call_names_the_token_programs_shards_in_order() {
    let call = plan();
    let ids: Vec<AccountId> = call.shard_selectors.iter().map(|s| s.account_id).collect();
    assert_eq!(
        ids,
        vec![
            definition_id(),
            crate::test_support::user_holding_id(),
            mint_authority_id()
        ]
    );
    assert!(call
        .shard_selectors
        .iter()
        .all(|s| s.program_account_id == TOKEN_PROGRAM_ID));
}

#[test]
fn chained_call_carries_the_authority_pda_seed() {
    assert_eq!(plan().pda_seeds.len(), 1, "the mint is seed-authorized");
}

#[test]
#[should_panic(expected = "Recipient authorization is missing")]
fn rejects_unauthorized_recipient() {
    let _ = plan_faucet_mint(
        &recipient_account(false),
        &allowance_slot(),
        &user_holding_slot(),
        &definition_slot(),
        &authority_slot(),
        &clock_slot(),
        TOKEN_MINT_AUTHORITY_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "Mint authority account ID does not match expected PDA derivation")]
fn rejects_wrong_authority_pda() {
    let _ = plan_faucet_mint(
        &recipient_account(true),
        &allowance_slot(),
        &user_holding_slot(),
        &definition_slot(),
        &authority_slot_at(AccountId::new([0x99; 32])),
        &clock_slot(),
        TOKEN_MINT_AUTHORITY_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "Mint allowance account ID does not match expected PDA derivation")]
fn rejects_wrong_allowance_pda() {
    let _ = plan_faucet_mint(
        &recipient_account(true),
        &allowance_slot_at(AccountId::new([0x98; 32])),
        &user_holding_slot(),
        &definition_slot(),
        &authority_slot(),
        &clock_slot(),
        TOKEN_MINT_AUTHORITY_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "Clock account must be the system CLOCK_01 account")]
fn rejects_wrong_clock_account() {
    let _ = plan_faucet_mint(
        &recipient_account(true),
        &allowance_slot(),
        &user_holding_slot(),
        &definition_slot(),
        &authority_slot(),
        &clock_slot_at(AccountId::new([0x97; 32])),
        TOKEN_MINT_AUTHORITY_PROGRAM_ID,
    );
}

// ── clock guard ─────────────────────────────────────────────────────────────

#[test]
fn check_clock_accepts_the_clocks_own_time() {
    check_clock(&clock_data(NOW), NOW);
}

/// The bound is one-directional: understating is allowed, since it can only make
/// the faucet more throttled.
#[test]
fn check_clock_accepts_an_understated_time() {
    check_clock(&clock_data(NOW), NOW - 1_000);
}

#[test]
#[should_panic(expected = "FaucetMint declared a time ahead of the clock")]
fn check_clock_rejects_an_overstated_time() {
    // Overstating is what would let a caller walk past the cooldown.
    check_clock(&clock_data(NOW), NOW + 1);
}

#[test]
#[should_panic(expected = "Clock account must be initialized")]
fn check_clock_rejects_an_empty_clock_shard() {
    check_clock(&empty(), NOW);
}

// ── definition guard ────────────────────────────────────────────────────────

#[test]
fn check_faucet_authority_accepts_our_pda() {
    check_faucet_authority(
        &faucet_definition(Some(mint_authority_id())),
        mint_authority_id(),
    );
}

#[test]
#[should_panic(expected = "Faucet token definition must be initialized")]
fn rejects_uninitialized_definition() {
    check_faucet_authority(&empty(), mint_authority_id());
}

#[test]
#[should_panic(expected = "Faucet token mint authority is not this program's mint-authority PDA")]
fn rejects_definition_whose_authority_is_not_our_pda() {
    check_faucet_authority(
        &faucet_definition(Some(AccountId::new([0x77; 32]))),
        mint_authority_id(),
    );
}

#[test]
#[should_panic(expected = "renounced mint authority")]
fn rejects_definition_with_renounced_authority() {
    check_faucet_authority(&faucet_definition(None), mint_authority_id());
}

#[test]
#[should_panic(expected = "Faucet token definition must be Fungible")]
fn rejects_non_fungible_definition() {
    check_faucet_authority(&non_fungible_definition(), mint_authority_id());
}

#[test]
#[should_panic(expected = "must decode as a TokenDefinition")]
fn rejects_a_definition_shard_that_is_not_a_definition() {
    check_faucet_authority(&holding_data(), mint_authority_id());
}

// ── allowance effect (the cooldown) ─────────────────────────────────────────

#[test]
fn first_mint_stamps_a_fresh_allowance() {
    let written = record_mint(&empty(), recipient_id(), definition_id(), NOW);
    assert_eq!(
        MintAllowance::try_from(&written).expect("a MintAllowance was written"),
        MintAllowance {
            recipient_id: recipient_id(),
            definition_id: definition_id(),
            last_mint_ms: NOW,
        }
    );
}

#[test]
fn mint_exactly_at_the_cooldown_boundary_is_allowed() {
    let written = record_mint(
        &allowance_data(NOW),
        recipient_id(),
        definition_id(),
        NOW + MINT_COOLDOWN_MS,
    );
    assert_eq!(
        MintAllowance::try_from(&written)
            .expect("a MintAllowance was written")
            .last_mint_ms,
        NOW + MINT_COOLDOWN_MS
    );
}

#[test]
#[should_panic(expected = "FaucetMint cooldown has not elapsed")]
fn rejects_a_second_mint_within_the_cooldown() {
    let _ = record_mint(
        &allowance_data(NOW),
        recipient_id(),
        definition_id(),
        NOW + MINT_COOLDOWN_MS - 1,
    );
}

#[test]
#[should_panic(expected = "FaucetMint cooldown has not elapsed")]
fn rejects_a_second_mint_at_the_same_instant() {
    let _ = record_mint(&allowance_data(NOW), recipient_id(), definition_id(), NOW);
}

#[test]
#[should_panic(expected = "FaucetMint cooldown has not elapsed")]
fn a_backwards_clock_stays_throttled() {
    let _ = record_mint(
        &allowance_data(NOW),
        recipient_id(),
        definition_id(),
        NOW - MINT_COOLDOWN_MS,
    );
}
