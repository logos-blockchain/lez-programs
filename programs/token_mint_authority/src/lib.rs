//! A permissionless, rate-limited faucet for the fungible token program.
//!
//! The recipient's actor first charges its allowance against the system clock. A separate
//! call to the destination's faucet actor mints the grant and verifies the credit locally.
//! Both calls settle in the same transaction.

use lee_core::program::{ReceiveInput, Response};
pub use token_mint_authority_core as core;
use token_mint_authority_core::{FaucetState, Internal, Message};

mod faucet_mint;
#[cfg(test)]
mod tests;

/// Dispatches faucet messages by their sender and the receiving actor's state.
pub fn handle_message(input: &ReceiveInput) -> Response {
    let state = FaucetState::decode(&input.pre_state);
    if input.from_own_program() {
        let message: Internal =
            borsh::from_slice(&input.message).expect("internal faucet message must decode");
        return match message {
            Internal::Claim => faucet_mint::read_clock(),
            Internal::Fund {
                token_program_id,
                definition,
                to,
            } => faucet_mint::fund(input, state, token_program_id, definition, to),
            Internal::AssertSettled => {
                // Fund schedules this check on its own actor. A private sender can
                // present a different address even on that final self-call.
                assert!(
                    matches!(state, None | Some(FaucetState::Allowance { .. })),
                    "faucet mint did not settle"
                );
                Response::keep_state()
            }
        };
    }
    if input.from == Some(clock_core::clock_actor()) {
        return faucet_mint::claim(input, state);
    }
    match state {
        Some(FaucetState::ReadingHolding {
            token_program_id,
            definition,
            to,
            last_mint_ms,
        }) => faucet_mint::mint(input, token_program_id, definition, to, last_mint_ms),
        Some(FaucetState::VerifyingCredit {
            token_program_id,
            definition,
            expected_balance,
            last_mint_ms,
        }) => faucet_mint::verify(
            input,
            token_program_id,
            definition,
            expected_balance,
            last_mint_ms,
        ),
        None | Some(FaucetState::Allowance { .. }) => {
            let message: Message =
                borsh::from_slice(&input.message).expect("faucet message must decode");
            match message {
                Message::FaucetMint {
                    token_program_id,
                    definition,
                    to,
                } => faucet_mint::request(input, token_program_id, definition, to),
            }
        }
    }
}
