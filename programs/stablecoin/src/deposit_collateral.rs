//! `DepositCollateral`: the owner adds collateral to an open position. Only the position is
//! involved, so it works while the protocol is frozen.
//!
//! The token program credits the vault with an async `IncreaseBalance`, and that credit is
//! what refuses a holding of another token ("recipient holds another token"). Neither the
//! position nor the engine can read what a caller-named holding holds, so the deposit must make
//! sure the credit is delivered inside this transaction, where a refusal fails the whole
//! deposit. LEZ delivers an async message to a declared public actor in the same transaction,
//! so the deposit first sends the vault a sync `InitializeHolding` of the collateral definition,
//! with the vault's seed: a sync message forces `vault@token` to be declared, and on the vault's
//! existing holding of that definition it changes nothing (a holding of another token is
//! refused with "account already holds another token state").

use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response, SyncMessage},
};
use stablecoin_core::{
    compute_position_vault_pda, compute_position_vault_pda_seed,
    event::{CollateralDeposited, StablecoinEvent as _},
};

use crate::{checks, position_state, program_id, save_position};

/// At the owner's actor, signed as a root message: count `amount` into the position, pin the
/// vault into the transaction with a seeded `InitializeHolding`, then pull `amount` from
/// `collateral_holding` into the vault. A deposit of zero changes nothing and sends nothing.
///
/// # Panics
/// - `"only the account's owner may send this"` when the message is nested or unsigned.
/// - `"this account has no position"` when the receiver holds no position.
/// - `"Position collateral_amount overflow"` when the sum exceeds `u128`.
pub(crate) fn deposit_collateral(
    input: &ReceiveInput,
    collateral_holding: AccountId,
    amount: u128,
) -> Response {
    checks::assert_owner(input);
    let mut position = position_state(input);
    if amount == 0 {
        return Response::keep_state();
    }
    position.collateral_amount = position
        .collateral_amount
        .checked_add(amount)
        .expect("Position collateral_amount overflow");
    let owner = input.receiver.account_id;
    let vault = compute_position_vault_pda(program_id(input), owner);
    let token_program_id = position.token_program_id;
    let collateral_definition_id = position.collateral_definition_id;
    save_position(position)
        .send(
            SyncMessage::new(
                Actor::new(vault, token_program_id),
                &token_core::Message::InitializeHolding {
                    definition: collateral_definition_id,
                },
            )
            .with_pda_seeds([compute_position_vault_pda_seed(owner)]),
        )
        .send_sync(
            Actor::new(collateral_holding, token_program_id),
            &token_core::Message::Transfer { to: vault, amount },
        )
        .event(CollateralDeposited { owner, amount }.to_program_event())
}

#[cfg(test)]
mod tests {
    use stablecoin_core::event::{CollateralDeposited, StablecoinEvent as _};

    use super::*;
    use crate::test_utils::*;

    fn position(collateral_amount: u128) -> StablecoinState {
        StablecoinState::Position(Position {
            token_program_id: TOKEN,
            collateral_definition_id: COLLATERAL_DEFINITION,
            collateral_amount,
            normalized_debt_amount: 7,
        })
    }

    fn deposit(
        from: Option<Actor>,
        is_authorized: bool,
        pre: Option<&StablecoinState>,
        amount: u128,
    ) -> Response {
        run(
            stablecoin(OWNER),
            from,
            is_authorized,
            pre,
            &Message::DepositCollateral {
                collateral_holding: HOLDING,
                amount,
            },
        )
    }

    #[test]
    fn position_a_deposit_adds_to_the_position_and_pulls_the_tokens_in() {
        assert_eq!(
            deposit(None, true, Some(&position(500)), 300),
            Response::set_state(position(800).encode())
                .send(
                    SyncMessage::new(
                        token(vault_id(OWNER)),
                        &token_core::Message::InitializeHolding {
                            definition: COLLATERAL_DEFINITION,
                        },
                    )
                    .with_pda_seeds([compute_position_vault_pda_seed(OWNER)])
                )
                .send_sync(
                    token(HOLDING),
                    &token_core::Message::Transfer {
                        to: vault_id(OWNER),
                        amount: 300,
                    },
                )
                .event(
                    CollateralDeposited {
                        owner: OWNER,
                        amount: 300,
                    }
                    .to_program_event()
                )
        );
    }

    #[test]
    fn position_a_deposit_of_zero_changes_nothing_and_moves_no_tokens() {
        assert_eq!(
            deposit(None, true, Some(&position(500)), 0),
            Response::keep_state()
        );
    }

    #[test]
    fn position_a_deposit_is_allowed_while_frozen() {
        // The position never asks the engine, so the protocol's frozen flag is not consulted.
        let response = deposit(None, true, Some(&position(500)), 1);
        assert_eq!(
            response.post_state,
            Response::set_state(position(501).encode()).post_state
        );
    }

    #[test]
    #[should_panic(expected = "this account has no position")]
    fn position_depositing_without_a_position_panics() {
        let _ = deposit(None, true, None, 300);
    }

    #[test]
    #[should_panic(expected = "Position collateral_amount overflow")]
    fn position_a_deposit_that_overflows_the_collateral_panics() {
        let _ = deposit(None, true, Some(&position(u128::MAX)), 1);
    }

    #[test]
    #[should_panic(expected = "only the account's owner may send this")]
    fn position_depositing_from_another_actor_panics() {
        let _ = deposit(
            Some(outsider(OTHER_PROGRAM)),
            true,
            Some(&position(500)),
            300,
        );
    }

    #[test]
    #[should_panic(expected = "only the account's owner may send this")]
    fn position_depositing_without_the_owners_signature_panics() {
        let _ = deposit(None, false, Some(&position(500)), 300);
    }
}
