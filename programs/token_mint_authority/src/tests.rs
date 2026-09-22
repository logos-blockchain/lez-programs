//! Unit tests for the `faucet_mint` host function.

#![allow(
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    reason = "tests deliberately panic on bad state via assert!/#[should_panic] and index fixed-size vectors"
)]

use crate::StateDiffExt;
use lee_core::{
    account::{Account, AccountId, AccountWithMetadata},
    program::{AccountStateDiff, ChainedCall},
};
use token_core::Instruction as TokenInstruction;
use token_mint_authority_core::{
    compute_mint_authority_pda_seed, MintAllowance, FAUCET_MINT_AMOUNT, MINT_COOLDOWN_MS,
};

use crate::{
    faucet_mint::faucet_mint,
    test_support::{
        allowance_account, clock_account, definition_id, faucet_definition_account,
        faucet_definition_with_authority, fresh_user_holding_account, mint_authority_account,
        mint_authority_id, recipient_account, recipient_id, uninitialized_allowance,
        user_holding_account, user_holding_id, NOW, TOKEN_MINT_AUTHORITY_PROGRAM_ID,
        TOKEN_PROGRAM_ID,
    },
};

fn invoke(allowance: AccountWithMetadata, now: u64) -> (Vec<AccountStateDiff>, Vec<ChainedCall>) {
    faucet_mint(
        recipient_account(),
        allowance,
        user_holding_account(),
        faucet_definition_account(),
        mint_authority_account(),
        clock_account(now),
        TOKEN_MINT_AUTHORITY_PROGRAM_ID,
    )
}

fn decode_token_instruction(call: &ChainedCall) -> TokenInstruction {
    borsh::from_slice::<TokenInstruction>(&call.instruction_data)
        .expect("chained instruction must decode as a token_core::Instruction")
}

#[test]
fn first_mint_returns_six_post_states_and_one_chained_call() {
    let (post_states, chained_calls) = invoke(uninitialized_allowance(), NOW);
    assert_eq!(post_states.len(), 6);
    assert_eq!(chained_calls.len(), 1);
}

#[test]
fn first_mint_claims_allowance_pda_and_stamps_now() {
    let (post_states, _) = invoke(uninitialized_allowance(), NOW);
    // post_states[1] is the allowance (input-order). Writing its data is the claim in
    // v0.2.5 — there is no `Claim::Pda` to inspect — so assert the ownership that write
    // acquires. The PDA address itself is asserted by `verify_mint_allowance_and_get_seed`.
    assert!(post_states[1].writes_data());
    assert_eq!(
        post_states[1].post_owner(TOKEN_MINT_AUTHORITY_PROGRAM_ID),
        TOKEN_MINT_AUTHORITY_PROGRAM_ID
    );
    let decoded = MintAllowance::try_from(post_states[1].post_data()).unwrap();
    assert_eq!(decoded.recipient_id, recipient_id());
    assert_eq!(decoded.definition_id, definition_id());
    assert_eq!(decoded.last_mint_ms, NOW);
}

#[test]
fn first_mint_echoes_authority_pda_without_claiming_it() {
    // The authority PDA holds no state; the seed alone authorizes the chained
    // mint, so this program takes no ownership of it.
    let (post_states, _) = invoke(uninitialized_allowance(), NOW);
    assert!(!post_states[4].writes_data());
    assert_eq!(
        post_states[4].post_owner(TOKEN_MINT_AUTHORITY_PROGRAM_ID),
        post_states[4].pre_state.account.program_owner
    );
}

#[test]
fn post_state_order_mirrors_inputs() {
    let (post_states, _) = invoke(uninitialized_allowance(), NOW);
    // [recipient, allowance, holding, definition, authority, clock]
    assert_eq!(
        post_states[0].post_account(TOKEN_MINT_AUTHORITY_PROGRAM_ID),
        Account::default()
    ); // recipient echoed
    assert_eq!(
        post_states[5].post_account(TOKEN_MINT_AUTHORITY_PROGRAM_ID),
        clock_account(NOW).account
    ); // clock echoed
}

#[test]
fn chained_call_delegates_fixed_mint_to_token_program() {
    let (_, chained_calls) = invoke(uninitialized_allowance(), NOW);
    let call = &chained_calls[0];

    assert_eq!(call.program_account_id, TOKEN_PROGRAM_ID);
    // MintWithAuthority account order: [definition, holding, authority]. Calls carry bare
    // account ids since v0.2.5; the authority PDA's authority comes from its seed alone.
    assert_eq!(
        call.pre_state_ids,
        vec![definition_id(), user_holding_id(), mint_authority_id()]
    );
    assert_eq!(call.pda_seeds, vec![compute_mint_authority_pda_seed()]);

    match decode_token_instruction(call) {
        TokenInstruction::MintWithAuthority { amount_to_mint } => {
            assert_eq!(amount_to_mint, FAUCET_MINT_AMOUNT);
        }
        _ => panic!("expected chained instruction to be Token::MintWithAuthority"),
    }
}

#[test]
fn mint_exactly_at_cooldown_boundary_is_allowed_and_rewrites_without_claim() {
    let (post_states, chained_calls) = invoke(allowance_account(NOW - MINT_COOLDOWN_MS), NOW);
    assert_eq!(chained_calls.len(), 1);
    // Already owned by this program -> rewritten, and the owner is unchanged.
    assert_eq!(
        post_states[1].post_owner(TOKEN_MINT_AUTHORITY_PROGRAM_ID),
        TOKEN_MINT_AUTHORITY_PROGRAM_ID
    );
    let decoded = MintAllowance::try_from(post_states[1].post_data()).unwrap();
    assert_eq!(decoded.last_mint_ms, NOW);
}

#[test]
fn mint_to_a_fresh_holding_delegates_the_authorized_holding() {
    let (post_states, chained_calls) = faucet_mint(
        recipient_account(),
        uninitialized_allowance(),
        fresh_user_holding_account(),
        faucet_definition_account(),
        mint_authority_account(),
        clock_account(NOW),
        TOKEN_MINT_AUTHORITY_PROGRAM_ID,
    );
    assert_eq!(chained_calls.len(), 1);
    // Holding is echoed as default; the chained mint materializes it.
    assert_eq!(
        post_states[2].post_account(TOKEN_MINT_AUTHORITY_PROGRAM_ID),
        Account::default()
    );
    assert_eq!(chained_calls[0].pre_state_ids[1], user_holding_id());
}

#[test]
#[should_panic(expected = "A fresh user holding must be authorized by its owner")]
fn rejects_creating_a_holding_the_caller_cannot_sign_for() {
    // An unowned address the caller names but nobody authorized. v0.2.4 rejected this
    // downstream via `Claim::Authorized`; v0.2.5 has no claims, so the faucet checks it.
    let unauthorized_fresh_holding = AccountWithMetadata {
        is_authorized: false,
        ..fresh_user_holding_account()
    };
    faucet_mint(
        recipient_account(),
        uninitialized_allowance(),
        unauthorized_fresh_holding,
        faucet_definition_account(),
        mint_authority_account(),
        clock_account(NOW),
        TOKEN_MINT_AUTHORITY_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "FaucetMint cooldown has not elapsed")]
fn rejects_a_second_mint_within_the_cooldown() {
    let _ = invoke(allowance_account(NOW - (MINT_COOLDOWN_MS - 1)), NOW);
}

#[test]
#[should_panic(expected = "FaucetMint cooldown has not elapsed")]
fn rejects_a_second_mint_at_the_same_instant() {
    let _ = invoke(allowance_account(NOW), NOW);
}

#[test]
#[should_panic(expected = "FaucetMint cooldown has not elapsed")]
fn a_backwards_clock_stays_throttled() {
    // Clock earlier than the last mint: saturating_sub -> 0 elapsed -> blocked.
    let _ = invoke(allowance_account(NOW), NOW - 1_000);
}

#[test]
#[should_panic(expected = "Recipient authorization is missing")]
fn rejects_unauthorized_recipient() {
    let mut recipient = recipient_account();
    recipient.is_authorized = false;
    let _ = faucet_mint(
        recipient,
        uninitialized_allowance(),
        user_holding_account(),
        faucet_definition_account(),
        mint_authority_account(),
        clock_account(NOW),
        TOKEN_MINT_AUTHORITY_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "Faucet token definition must be initialized")]
fn rejects_uninitialized_definition() {
    let definition = AccountWithMetadata {
        account: Account::default(),
        is_authorized: false,
        account_id: definition_id(),
    };
    let _ = faucet_mint(
        recipient_account(),
        uninitialized_allowance(),
        user_holding_account(),
        definition,
        mint_authority_account(),
        clock_account(NOW),
        TOKEN_MINT_AUTHORITY_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "Faucet token mint authority is not this program's mint-authority PDA")]
fn rejects_definition_whose_authority_is_not_our_pda() {
    let _ = faucet_mint(
        recipient_account(),
        uninitialized_allowance(),
        user_holding_account(),
        faucet_definition_with_authority(Some(AccountId::new([0xEE; 32]))),
        mint_authority_account(),
        clock_account(NOW),
        TOKEN_MINT_AUTHORITY_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "renounced mint authority")]
fn rejects_definition_with_renounced_authority() {
    let _ = faucet_mint(
        recipient_account(),
        uninitialized_allowance(),
        user_holding_account(),
        faucet_definition_with_authority(None),
        mint_authority_account(),
        clock_account(NOW),
        TOKEN_MINT_AUTHORITY_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "Mint authority account ID does not match expected PDA derivation")]
fn rejects_wrong_authority_pda() {
    let mut authority = mint_authority_account();
    authority.account_id = AccountId::new([0xEE; 32]);
    let _ = faucet_mint(
        recipient_account(),
        uninitialized_allowance(),
        user_holding_account(),
        faucet_definition_account(),
        authority,
        clock_account(NOW),
        TOKEN_MINT_AUTHORITY_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "Mint allowance account ID does not match expected PDA derivation")]
fn rejects_wrong_allowance_pda() {
    let mut allowance = uninitialized_allowance();
    allowance.account_id = AccountId::new([0xEE; 32]);
    let _ = invoke(allowance, NOW);
}

#[test]
#[should_panic(expected = "Mint allowance account is not owned by this program")]
fn rejects_foreign_owned_allowance() {
    let mut allowance = allowance_account(NOW - MINT_COOLDOWN_MS);
    allowance.account.program_owner = AccountId::new([9u8; 32]);
    let _ = invoke(allowance, NOW);
}

#[test]
#[should_panic(expected = "Clock account must be the system CLOCK_01 account")]
fn rejects_wrong_clock_account() {
    let mut clock = clock_account(NOW);
    clock.account_id = AccountId::new([0xC1; 32]);
    let _ = faucet_mint(
        recipient_account(),
        uninitialized_allowance(),
        user_holding_account(),
        faucet_definition_account(),
        mint_authority_account(),
        clock,
        TOKEN_MINT_AUTHORITY_PROGRAM_ID,
    );
}
