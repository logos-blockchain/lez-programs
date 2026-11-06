//! `NewDefinition`: the definition writes itself and initializes the holding of its supply.

use lee_core::{
    account::AccountId,
    program::{ReceiveInput, Response},
};
use token_core::{
    event::{NewDefinition, TokenEvent as _},
    Internal, TokenDefinition, TokenState, MAX_NAME_LEN,
};

use crate::{assert_not_zero, token_actor};

/// Any sender: the AMM creates its LP definition on a PDA it authorizes with a seed.
pub(crate) fn new_definition(
    input: &ReceiveInput,
    name: String,
    total_supply: u128,
    mint_authority: Option<AccountId>,
    holding: AccountId,
) -> Response {
    assert!(
        input.pre_state.is_empty(),
        "definition account must be empty"
    );
    assert!(
        input.is_authorized,
        "definition account must authorize its creation"
    );
    assert!(
        !name.is_empty() && name.len() <= MAX_NAME_LEN,
        "token name must be 1 to {MAX_NAME_LEN} bytes"
    );
    assert_not_zero(mint_authority);
    let definition = input.receiver.account_id;
    assert!(
        holding != definition,
        "the initial holding must be another account than the definition"
    );

    let event = NewDefinition {
        definition,
        holding,
        name: name.clone(),
        total_supply,
        authority: mint_authority,
    };
    let state = TokenState::Definition(TokenDefinition {
        name,
        total_supply,
        authority: mint_authority,
    });
    Response::set_state(state.encode())
        .send_sync(
            token_actor(input, holding),
            &Internal::InitHolding {
                definition,
                amount: total_supply,
            },
        )
        .event(event.to_program_event())
}

#[cfg(test)]
mod tests {
    use crate::test_utils::*;

    fn message(name: &str, mint_authority: Option<AccountId>, holding: AccountId) -> Message {
        Message::NewDefinition {
            name: name.to_owned(),
            total_supply: 1_000,
            mint_authority,
            holding,
        }
    }

    #[test]
    fn writes_the_definition_and_initializes_the_holding_with_the_supply() {
        let response = run(
            DEFINITION,
            None,
            true,
            None,
            &message("Test", Some(AUTHORITY), HOLDING),
        );
        let expected = write(&definition(1_000, Some(AUTHORITY)))
            .send_sync(
                token(HOLDING),
                &Internal::InitHolding {
                    definition: DEFINITION,
                    amount: 1_000,
                },
            )
            .event(
                NewDefinition {
                    definition: DEFINITION,
                    holding: HOLDING,
                    name: "Test".to_owned(),
                    total_supply: 1_000,
                    authority: Some(AUTHORITY),
                }
                .to_program_event(),
            );
        assert_eq!(response, expected);
    }

    /// The AMM creates its LP definition on a PDA it authorizes with a seed.
    #[test]
    fn may_be_sent_by_another_program() {
        let response = run(
            DEFINITION,
            Some(outsider(AUTHORITY)),
            true,
            None,
            &message("Test", None, HOLDING),
        );
        assert_eq!(
            response.post_state,
            Some(ActorState::from(definition(1_000, None).encode()))
        );
    }

    #[test]
    fn accepts_a_name_of_the_maximum_length() {
        let name = "a".repeat(MAX_NAME_LEN);
        let response = run(DEFINITION, None, true, None, &message(&name, None, HOLDING));
        assert!(response.post_state.is_some());
    }

    #[test]
    #[should_panic(expected = "token name must be 1 to 64 bytes")]
    fn refuses_an_empty_name() {
        let _ = run(DEFINITION, None, true, None, &message("", None, HOLDING));
    }

    #[test]
    #[should_panic(expected = "token name must be 1 to 64 bytes")]
    fn refuses_a_name_one_byte_too_long() {
        let name = "a".repeat(MAX_NAME_LEN + 1);
        let _ = run(DEFINITION, None, true, None, &message(&name, None, HOLDING));
    }

    #[test]
    #[should_panic(expected = "definition account must authorize its creation")]
    fn refuses_an_unauthorized_definition() {
        let _ = run(
            DEFINITION,
            None,
            false,
            None,
            &message("Test", None, HOLDING),
        );
    }

    #[test]
    #[should_panic(expected = "definition account must be empty")]
    fn refuses_an_existing_definition() {
        let _ = run(
            DEFINITION,
            None,
            true,
            Some(&definition(1, None)),
            &message("Test", None, HOLDING),
        );
    }

    #[test]
    #[should_panic(expected = "definition account must be empty")]
    fn refuses_an_existing_holding() {
        let _ = run(
            DEFINITION,
            None,
            true,
            Some(&holding(OTHER_DEFINITION, 0)),
            &message("Test", None, HOLDING),
        );
    }

    #[test]
    #[should_panic(expected = "mint authority must not be the zero account id")]
    fn refuses_the_zero_account_as_authority() {
        let _ = run(
            DEFINITION,
            None,
            true,
            None,
            &message("Test", Some(ZERO), HOLDING),
        );
    }

    #[test]
    #[should_panic(expected = "the initial holding must be another account than the definition")]
    fn refuses_the_definition_as_its_own_holding() {
        let _ = run(
            DEFINITION,
            None,
            true,
            None,
            &message("Test", None, DEFINITION),
        );
    }
}
