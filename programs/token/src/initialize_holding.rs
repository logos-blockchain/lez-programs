//! `InitializeHolding`: holding → definition → holding (`token.md` §4.3), so a holding only
//! ever exists for a definition that does.
//!
//! The first message goes to the holding because a seed grant authorizes only the account it
//! is addressed to; the holding's authorization then travels down the sync chain to the
//! definition and back (`docs/actor-model/initialize-holding.md`).

use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response},
};
use token_core::{Internal, TokenHolding, TokenState};

use crate::{definition_state, token_actor};

/// At the holding: ask `definition` to initialize it.
pub(crate) fn initialize_holding(input: &ReceiveInput, definition: AccountId) -> Response {
    assert!(
        input.is_authorized,
        "holding account must authorize its initialization"
    );
    match TokenState::decode(&input.pre_state) {
        None => Response::keep_state()
            .send_sync(token_actor(input, definition), &Internal::RequestHolding),
        // Idempotent, so ATA `Create` can be repeated.
        Some(TokenState::Holding(holding)) if holding.definition_id == definition => {
            Response::keep_state()
        }
        Some(TokenState::Holding(_) | TokenState::Definition(_)) => {
            panic!("account already holds another token state")
        }
    }
}

/// At the definition: prove it exists by answering. It never takes a target from the message.
pub(crate) fn request_holding(input: &ReceiveInput, from: Actor) -> Response {
    let _ = definition_state(input);
    Response::keep_state().send_sync(
        from,
        &Internal::InitHolding {
            definition: input.receiver.account_id,
            amount: 0,
        },
    )
}

/// At the holding, from its definition: write it. Also the last step of `NewDefinition`.
pub(crate) fn init_holding(
    input: &ReceiveInput,
    from: Actor,
    definition: AccountId,
    amount: u128,
) -> Response {
    assert!(input.pre_state.is_empty(), "holding account must be empty");
    assert!(
        input.is_authorized,
        "holding account must authorize its initialization"
    );
    assert!(
        from.account_id == definition,
        "only the definition initializes its holdings"
    );
    Response::set_state(
        TokenState::Holding(TokenHolding {
            definition_id: definition,
            balance: amount,
        })
        .encode(),
    )
}

#[cfg(test)]
mod tests {
    mod initialize_holding {
        use crate::test_utils::*;

        fn message(definition: AccountId) -> Message {
            Message::InitializeHolding { definition }
        }

        #[test]
        fn asks_the_definition_without_writing() {
            let response = run(HOLDING, None, true, None, &message(DEFINITION));
            assert_eq!(
                response,
                Response::keep_state().send_sync(token(DEFINITION), &Internal::RequestHolding)
            );
        }

        /// ATA forwards with its seed: the token program only sees `is_authorized`.
        #[test]
        fn may_be_sent_by_another_program() {
            let response = run(
                HOLDING,
                Some(outsider(AUTHORITY)),
                true,
                None,
                &message(DEFINITION),
            );
            assert_eq!(
                response,
                Response::keep_state().send_sync(token(DEFINITION), &Internal::RequestHolding)
            );
        }

        #[test]
        fn is_a_no_op_on_a_holding_of_the_same_definition() {
            let response = run(
                HOLDING,
                None,
                true,
                Some(&holding(DEFINITION, 7)),
                &message(DEFINITION),
            );
            assert_eq!(response, Response::keep_state());
        }

        #[test]
        #[should_panic(expected = "account already holds another token state")]
        fn refuses_a_holding_of_another_definition() {
            let _ = run(
                HOLDING,
                None,
                true,
                Some(&holding(OTHER_DEFINITION, 0)),
                &message(DEFINITION),
            );
        }

        #[test]
        #[should_panic(expected = "account already holds another token state")]
        fn refuses_a_definition() {
            let _ = run(
                HOLDING,
                None,
                true,
                Some(&definition(1, None)),
                &message(DEFINITION),
            );
        }

        #[test]
        #[should_panic(expected = "holding account must authorize its initialization")]
        fn refuses_an_unauthorized_holding() {
            let _ = run(HOLDING, None, false, None, &message(DEFINITION));
        }
    }

    mod request_holding {
        use crate::test_utils::*;

        #[test]
        fn the_definition_replies_with_a_zero_holding_to_the_sender() {
            let response = run(
                DEFINITION,
                Some(token(HOLDING)),
                false,
                Some(&definition(1_000, None)),
                &Internal::RequestHolding,
            );
            assert_eq!(
                response,
                Response::keep_state().send_sync(
                    token(HOLDING),
                    &Internal::InitHolding {
                        definition: DEFINITION,
                        amount: 0,
                    }
                )
            );
        }

        #[test]
        #[should_panic(expected = "receiver is not a token definition")]
        fn fails_at_a_holding() {
            let _ = run(
                OTHER_DEFINITION,
                Some(token(HOLDING)),
                false,
                Some(&holding(DEFINITION, 0)),
                &Internal::RequestHolding,
            );
        }

        #[test]
        #[should_panic(expected = "receiver is not a token definition")]
        fn fails_at_an_empty_actor() {
            let _ = run(
                OTHER_DEFINITION,
                Some(token(HOLDING)),
                false,
                None,
                &Internal::RequestHolding,
            );
        }
    }

    mod init_holding {
        use crate::test_utils::*;

        fn message(amount: u128) -> Internal {
            Internal::InitHolding {
                definition: DEFINITION,
                amount,
            }
        }

        #[test]
        fn writes_the_holding() {
            let response = run(
                HOLDING,
                Some(token(DEFINITION)),
                true,
                None,
                &message(1_000),
            );
            assert_eq!(response, write(&holding(DEFINITION, 1_000)));
        }

        #[test]
        #[should_panic(expected = "holding account must be empty")]
        fn refuses_an_existing_holding() {
            let _ = run(
                HOLDING,
                Some(token(DEFINITION)),
                true,
                Some(&holding(DEFINITION, 5)),
                &message(0),
            );
        }

        #[test]
        #[should_panic(expected = "holding account must authorize its initialization")]
        fn refuses_an_unauthorized_holding() {
            let _ = run(HOLDING, Some(token(DEFINITION)), false, None, &message(0));
        }

        #[test]
        #[should_panic(expected = "only the definition initializes its holdings")]
        fn refuses_another_token_actor() {
            let _ = run(HOLDING, Some(token(RECIPIENT)), true, None, &message(1_000));
        }
    }
}
