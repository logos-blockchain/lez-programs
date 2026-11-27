//! One grant, from the signed recipient through its allowance and destination holding.

use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response, StateReply, SyncMessage},
};
use token_core::{Message as TokenMessage, TokenState};
use token_mint_authority_core::{
    compute_mint_allowance_pda_seed, compute_mint_authority_pda_seed, FaucetState, Internal,
    FAUCET_MINT_AMOUNT, MINT_COOLDOWN_MS,
};

pub(crate) fn request(
    input: &ReceiveInput,
    token_program_id: AccountId,
    definition: AccountId,
    to: AccountId,
) -> Response {
    assert!(input.is_authorized, "recipient must authorize faucet mint");
    let faucet = input.receiver.program_account_id;
    assert!(
        token_program_id != faucet && token_program_id != clock_core::clock_account_id(),
        "token program must be distinct from the faucet and clock"
    );
    let seed =
        compute_mint_allowance_pda_seed(token_program_id, input.receiver.account_id, definition);
    let allowance = Actor::new(AccountId::for_public_pda(&faucet, &seed), faucet);
    // The allowance grant ends with the clock read. The token program receives only the
    // mint authority grant specific to its deployment, further down the second call.
    Response::keep_state()
        .send(SyncMessage::new(allowance, &Internal::Claim).with_pda_seeds([seed]))
        .send_sync(
            Actor::new(to, faucet),
            &Internal::Fund {
                token_program_id,
                definition,
                to,
            },
        )
}

pub(crate) fn read_clock() -> Response {
    Response::keep_state().send_sync(
        clock_core::clock_actor(),
        &clock_core::Message::ReadTimestamp { granularity: 1 },
    )
}

pub(crate) fn claim(input: &ReceiveInput, state: Option<FaucetState>) -> Response {
    let clock_core::Reply::Timestamp(now) = clock_core::Reply::from_message(&input.message) else {
        panic!("faucet requires a clock timestamp");
    };
    if let Some(previous) = idle_timestamp(state) {
        assert!(
            now.saturating_sub(previous) >= MINT_COOLDOWN_MS,
            "FaucetMint cooldown has not elapsed"
        );
    }
    Response::set_state(FaucetState::Allowance { last_mint_ms: now }.encode())
}

// Actor roles can coincide: for example, a token holding can live at an allowance PDA.
// Restore the allowance's timestamp when the transient balance checks finish.
fn idle_timestamp(state: Option<FaucetState>) -> Option<u64> {
    match state {
        None => None,
        Some(FaucetState::Allowance { last_mint_ms }) => Some(last_mint_ms),
        Some(FaucetState::ReadingHolding { .. } | FaucetState::VerifyingCredit { .. }) => {
            panic!("faucet mint already in progress")
        }
    }
}

pub(crate) fn fund(
    input: &ReceiveInput,
    state: Option<FaucetState>,
    token_program_id: AccountId,
    definition: AccountId,
    to: AccountId,
) -> Response {
    let holding = Actor::new(input.receiver.account_id, token_program_id);
    let pending = FaucetState::ReadingHolding {
        token_program_id,
        definition,
        to,
        last_mint_ms: idle_timestamp(state),
    };
    let mut response = Response::set_state(pending.encode());
    if input.is_authorized {
        // Idempotent for a holding of this definition. Only its owner may open a fresh one.
        response = response.send_sync(holding, &TokenMessage::InitializeHolding { definition });
    }
    response
        .send_sync(holding, &TokenMessage::ReadState)
        .send_sync(input.receiver, &Internal::AssertSettled)
}

fn holding_balance(
    input: &ReceiveInput,
    token_program_id: AccountId,
    definition: AccountId,
) -> u128 {
    let holding_actor = Actor::new(input.receiver.account_id, token_program_id);
    assert_eq!(
        input.from,
        Some(holding_actor),
        "unexpected holding state sender"
    );
    let reply: StateReply =
        borsh::from_slice(&input.message).expect("holding state reply must decode");
    let Some(TokenState::Holding(holding)) = TokenState::decode(&reply.state) else {
        panic!("destination must be an initialized token holding");
    };
    assert_eq!(
        holding.definition_id, definition,
        "destination holds another token"
    );
    holding.balance
}

pub(crate) fn mint(
    input: &ReceiveInput,
    token_program_id: AccountId,
    definition: AccountId,
    to: AccountId,
    last_mint_ms: Option<u64>,
) -> Response {
    let expected_balance = holding_balance(input, token_program_id, definition)
        .checked_add(FAUCET_MINT_AMOUNT)
        .expect("destination balance overflow");
    let seed = compute_mint_authority_pda_seed(token_program_id);
    let authority = AccountId::for_public_pda(&input.receiver.program_account_id, &seed);
    // Preserve the address supplied by the claimant when the holding is private.
    let message = TokenMessage::Mint {
        definition,
        to,
        amount: FAUCET_MINT_AMOUNT,
    };
    Response::set_state(
        FaucetState::VerifyingCredit {
            token_program_id,
            definition,
            expected_balance,
            last_mint_ms,
        }
        .encode(),
    )
    .send(
        SyncMessage::new(Actor::new(authority, token_program_id), &message).with_pda_seeds([seed]),
    )
    .send_sync(
        Actor::new(input.receiver.account_id, token_program_id),
        &TokenMessage::ReadState,
    )
}

pub(crate) fn verify(
    input: &ReceiveInput,
    token_program_id: AccountId,
    definition: AccountId,
    expected_balance: u128,
    last_mint_ms: Option<u64>,
) -> Response {
    assert_eq!(
        holding_balance(input, token_program_id, definition),
        expected_balance,
        "faucet credit did not settle"
    );
    Response::set_state(last_mint_ms.map_or_else(Vec::new, |last_mint_ms| {
        FaucetState::Allowance { last_mint_ms }.encode()
    }))
}
