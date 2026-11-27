//! Message-level invariants for grant scope, clock handling and settlement callbacks.

use borsh::BorshSerialize;
use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response, StateReply, SyncMessage},
};
use token_core::{TokenHolding, TokenState};
use token_mint_authority_core::{
    compute_mint_allowance_pda, compute_mint_allowance_pda_seed, compute_mint_authority_pda,
    compute_mint_authority_pda_seed, FaucetState, Internal, Message, FAUCET_MINT_AMOUNT,
};

use crate::handle_message;

const FAUCET: AccountId = AccountId::new([1; 32]);
const TOKEN: AccountId = AccountId::new([2; 32]);
const RECIPIENT: AccountId = AccountId::new([3; 32]);
const HOLDING: AccountId = AccountId::new([4; 32]);
const DEFINITION: AccountId = AccountId::new([5; 32]);

fn run(
    account: AccountId,
    from: Option<Actor>,
    authorized: bool,
    state: Option<FaucetState>,
    message: &impl BorshSerialize,
) -> Response {
    handle_message(&ReceiveInput {
        receiver: Actor::new(account, FAUCET),
        from,
        is_authorized: authorized,
        pre_state: state.map_or_else(Vec::new, |s| s.encode()).into(),
        message: borsh::to_vec(message).expect("test message encodes"),
    })
}

fn request(token_program_id: AccountId) -> Message {
    Message::FaucetMint {
        token_program_id,
        definition: DEFINITION,
        to: HOLDING,
    }
}

#[test]
fn only_the_allowance_call_receives_its_seed() {
    let allowance = compute_mint_allowance_pda(FAUCET, TOKEN, RECIPIENT, DEFINITION);
    let seed = compute_mint_allowance_pda_seed(TOKEN, RECIPIENT, DEFINITION);
    let expected = Response::keep_state()
        .send(
            SyncMessage::new(Actor::new(allowance, FAUCET), &Internal::Claim)
                .with_pda_seeds([seed]),
        )
        .send_sync(
            Actor::new(HOLDING, FAUCET),
            &Internal::Fund {
                token_program_id: TOKEN,
                definition: DEFINITION,
                to: HOLDING,
            },
        );
    assert_eq!(run(RECIPIENT, None, true, None, &request(TOKEN)), expected);
}

#[test]
#[should_panic(expected = "token program must be distinct")]
fn rejects_self_dispatch_as_the_token_program() {
    let _ = run(RECIPIENT, None, true, None, &request(FAUCET));
}

#[test]
#[should_panic(expected = "FaucetMint cooldown has not elapsed")]
fn a_backwards_clock_keeps_the_allowance_closed() {
    let allowance = compute_mint_allowance_pda(FAUCET, TOKEN, RECIPIENT, DEFINITION);
    let _ = run(
        allowance,
        Some(clock_core::clock_actor()),
        false,
        Some(FaucetState::Allowance { last_mint_ms: 1 }),
        &clock_core::Reply::Timestamp(0),
    );
}

fn reading(to: AccountId) -> FaucetState {
    FaucetState::ReadingHolding {
        token_program_id: TOKEN,
        definition: DEFINITION,
        to,
        last_mint_ms: None,
    }
}

fn holding_reply(balance: u128) -> StateReply {
    StateReply {
        state: TokenState::Holding(TokenHolding {
            definition_id: DEFINITION,
            balance,
        })
        .encode()
        .into(),
    }
}

#[test]
fn mint_preserves_the_requested_address_and_grants_only_its_token_authority() {
    let presented = AccountId::new([9; 32]);
    let token_holding = Actor::new(HOLDING, TOKEN);
    let mint = run(
        HOLDING,
        Some(token_holding),
        false,
        Some(reading(presented)),
        &holding_reply(7),
    );
    let authority = compute_mint_authority_pda(FAUCET, TOKEN);
    let seed = compute_mint_authority_pda_seed(TOKEN);
    assert_eq!(
        mint,
        Response::set_state(
            FaucetState::VerifyingCredit {
                token_program_id: TOKEN,
                definition: DEFINITION,
                expected_balance: 7 + FAUCET_MINT_AMOUNT,
                last_mint_ms: None,
            }
            .encode()
        )
        .send(
            SyncMessage::new(
                Actor::new(authority, TOKEN),
                &token_core::Message::Mint {
                    definition: DEFINITION,
                    to: presented,
                    amount: FAUCET_MINT_AMOUNT,
                }
            )
            .with_pda_seeds([seed])
        )
        .send_sync(token_holding, &token_core::Message::ReadState)
    );
}

#[test]
#[should_panic(expected = "unexpected holding state sender")]
fn a_different_holding_cannot_answer_the_balance_read() {
    let _ = run(
        HOLDING,
        Some(Actor::new(RECIPIENT, TOKEN)),
        false,
        Some(reading(HOLDING)),
        &holding_reply(0),
    );
}

#[test]
#[should_panic(expected = "faucet mint did not settle")]
fn a_missing_balance_reply_aborts_the_claim() {
    let _ = run(
        HOLDING,
        Some(Actor::new(HOLDING, FAUCET)),
        false,
        Some(reading(HOLDING)),
        &Internal::AssertSettled,
    );
}

#[test]
fn settling_a_holding_on_an_allowance_preserves_its_cooldown() {
    let state = FaucetState::VerifyingCredit {
        token_program_id: TOKEN,
        definition: DEFINITION,
        expected_balance: FAUCET_MINT_AMOUNT,
        last_mint_ms: Some(100),
    };
    let result = run(
        HOLDING,
        Some(Actor::new(HOLDING, TOKEN)),
        false,
        Some(state),
        &holding_reply(FAUCET_MINT_AMOUNT),
    );
    assert_eq!(
        result,
        Response::set_state(FaucetState::Allowance { last_mint_ms: 100 }.encode())
    );
}
