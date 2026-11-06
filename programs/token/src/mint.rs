//! `Mint`: at the definition when it is its own authority, otherwise through the authority's
//! own actor, which forwards `IncreaseSupply` (`token.md` §3.6).

use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response},
};
use token_core::{
    event::{Minted, TokenEvent as _},
    Internal, TokenDefinition, TokenState,
};

use crate::{assert_authority, definition_state, forward_as_authority, token_actor};

/// At the definition (self authority) or at the authority's actor.
pub(crate) fn mint(
    input: &ReceiveInput,
    definition: AccountId,
    to: AccountId,
    amount: u128,
) -> Response {
    if definition == input.receiver.account_id {
        assert!(
            input.is_authorized,
            "definition must authorize minting as its own authority"
        );
        let state = definition_state(input);
        assert_authority(&state, definition);
        raise_supply(input, state, to, amount)
    } else {
        forward_as_authority(input, definition, &Internal::IncreaseSupply { to, amount })
    }
}

/// At the definition, from its authority's actor.
pub(crate) fn increase_supply(
    input: &ReceiveInput,
    from: Actor,
    to: AccountId,
    amount: u128,
) -> Response {
    let definition = definition_state(input);
    assert_authority(&definition, from.account_id);
    raise_supply(input, definition, to, amount)
}

fn raise_supply(
    input: &ReceiveInput,
    mut definition: TokenDefinition,
    to: AccountId,
    amount: u128,
) -> Response {
    definition.total_supply = definition
        .total_supply
        .checked_add(amount)
        .expect("total supply overflow");
    let definition_id = input.receiver.account_id;
    Response::set_state(TokenState::Definition(definition).encode())
        .send_async(
            token_actor(input, to),
            &Internal::IncreaseBalance {
                definition: definition_id,
                amount,
            },
        )
        .event(
            Minted {
                definition: definition_id,
                to,
                amount,
            }
            .to_program_event(),
        )
}

#[cfg(test)]
mod tests {
    use crate::test_utils::*;

    fn message(amount: u128) -> Message {
        Message::Mint {
            definition: DEFINITION,
            to: RECIPIENT,
            amount,
        }
    }

    fn increase_balance(amount: u128) -> Internal {
        Internal::IncreaseBalance {
            definition: DEFINITION,
            amount,
        }
    }

    fn minted(amount: u128) -> ProgramEvent {
        Minted {
            definition: DEFINITION,
            to: RECIPIENT,
            amount,
        }
        .to_program_event()
    }

    #[test]
    fn a_self_authority_definition_mints() {
        let response = run(
            DEFINITION,
            None,
            true,
            Some(&definition(1_000, Some(DEFINITION))),
            &message(50),
        );
        assert_eq!(
            response,
            write(&definition(1_050, Some(DEFINITION)))
                .send_async(token(RECIPIENT), &increase_balance(50))
                .event(minted(50))
        );
    }

    #[test]
    #[should_panic(expected = "definition must authorize minting as its own authority")]
    fn a_self_authority_definition_must_be_authorized() {
        let _ = run(
            DEFINITION,
            None,
            false,
            Some(&definition(1_000, Some(DEFINITION))),
            &message(50),
        );
    }

    #[test]
    #[should_panic(expected = "sender is not the mint authority")]
    fn a_definition_with_an_external_authority_does_not_mint_itself() {
        let _ = run(
            DEFINITION,
            None,
            true,
            Some(&definition(1_000, Some(AUTHORITY))),
            &message(50),
        );
    }

    #[test]
    #[should_panic(expected = "mint authority is renounced")]
    fn a_fixed_supply_definition_does_not_mint() {
        let _ = run(
            DEFINITION,
            None,
            true,
            Some(&definition(1_000, None)),
            &message(50),
        );
    }

    #[test]
    #[should_panic(expected = "total supply overflow")]
    fn refuses_a_supply_overflow() {
        let _ = run(
            DEFINITION,
            None,
            true,
            Some(&definition(u128::MAX, Some(DEFINITION))),
            &message(1),
        );
    }

    /// The authority's actor keeps its state, whatever it is, and forwards.
    #[test]
    fn the_authority_actor_forwards() {
        for pre in [None, Some(holding(OTHER_DEFINITION, 3))] {
            let response = run(AUTHORITY, None, true, pre.as_ref(), &message(50));
            assert_eq!(
                response,
                Response::keep_state().send_sync(
                    token(DEFINITION),
                    &Internal::IncreaseSupply {
                        to: RECIPIENT,
                        amount: 50,
                    }
                )
            );
        }
    }

    #[test]
    #[should_panic(expected = "mint authority must authorize")]
    fn the_authority_actor_must_be_authorized() {
        let _ = run(AUTHORITY, None, false, None, &message(50));
    }

    #[test]
    fn the_definition_mints_for_its_authority() {
        let response = run(
            DEFINITION,
            Some(token(AUTHORITY)),
            false,
            Some(&definition(1_000, Some(AUTHORITY))),
            &Internal::IncreaseSupply {
                to: RECIPIENT,
                amount: 50,
            },
        );
        assert_eq!(
            response,
            write(&definition(1_050, Some(AUTHORITY)))
                .send_async(token(RECIPIENT), &increase_balance(50))
                .event(minted(50))
        );
    }

    #[test]
    #[should_panic(expected = "sender is not the mint authority")]
    fn the_definition_refuses_another_token_actor() {
        let _ = run(
            DEFINITION,
            Some(token(NEW_AUTHORITY)),
            false,
            Some(&definition(1_000, Some(AUTHORITY))),
            &Internal::IncreaseSupply {
                to: RECIPIENT,
                amount: 50,
            },
        );
    }

    #[test]
    #[should_panic(expected = "mint authority is renounced")]
    fn the_definition_refuses_after_renouncing() {
        let _ = run(
            DEFINITION,
            Some(token(AUTHORITY)),
            false,
            Some(&definition(1_000, None)),
            &Internal::IncreaseSupply {
                to: RECIPIENT,
                amount: 50,
            },
        );
    }
}
