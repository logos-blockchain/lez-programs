//! `Transfer`: the sender debits itself and credits the recipient asynchronously. The credit
//! carries no authorization (a seed grant on the sender stays behind), and for a private
//! recipient later it can wait in the mailbox.

use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response},
};
use token_core::{
    event::{TokenEvent as _, Transferred},
    Internal, TokenState,
};

use crate::{holding_state, token_actor};

/// At the sender's holding.
pub(crate) fn transfer(input: &ReceiveInput, to: AccountId, amount: u128) -> Response {
    assert!(
        input.is_authorized,
        "sender holding must authorize the transfer"
    );
    let mut holding = holding_state(input);
    holding.balance = holding
        .balance
        .checked_sub(amount)
        .expect("insufficient balance");
    let definition = holding.definition_id;
    Response::set_state(TokenState::Holding(holding).encode()).send_async(
        token_actor(input, to),
        &Internal::IncreaseBalance { definition, amount },
    )
}

/// At the recipient, for a transfer or a mint. Lands only on an existing holding of the same
/// definition, so nobody can occupy an account's token actor with a token its owner didn't
/// choose.
pub(crate) fn increase_balance(
    input: &ReceiveInput,
    from: Actor,
    definition: AccountId,
    amount: u128,
) -> Response {
    let mut holding = match TokenState::decode(&input.pre_state) {
        Some(TokenState::Holding(holding)) => holding,
        None => panic!("recipient has no token holding; initialize it first"),
        Some(TokenState::Definition(_)) => panic!("recipient is a token definition"),
    };
    assert!(
        holding.definition_id == definition,
        "recipient holds another token"
    );
    holding.balance = holding
        .balance
        .checked_add(amount)
        .expect("balance overflow");

    let response = Response::set_state(TokenState::Holding(holding).encode());
    // A credit from the definition is a mint, which the definition announced as `Minted`.
    if from.account_id == definition {
        return response;
    }
    response.event(
        Transferred {
            definition,
            from: from.account_id,
            to: input.receiver.account_id,
            amount,
        }
        .to_program_event(),
    )
}

#[cfg(test)]
mod tests {
    mod transfer {
        use crate::test_utils::*;

        fn message(amount: u128) -> Message {
            Message::Transfer {
                to: RECIPIENT,
                amount,
            }
        }

        #[test]
        fn debits_and_credits_the_recipient_async() {
            let response = run(
                HOLDING,
                None,
                true,
                Some(&holding(DEFINITION, 100)),
                &message(40),
            );
            let expected = write(&holding(DEFINITION, 60)).send_async(
                token(RECIPIENT),
                &Internal::IncreaseBalance {
                    definition: DEFINITION,
                    amount: 40,
                },
            );
            assert_eq!(response, expected);
            assert!(
                response.sync_messages.is_empty(),
                "no grant may travel with the credit"
            );
        }

        #[test]
        #[should_panic(expected = "sender holding must authorize the transfer")]
        fn refuses_an_unauthorized_sender() {
            let _ = run(
                HOLDING,
                None,
                false,
                Some(&holding(DEFINITION, 100)),
                &message(40),
            );
        }

        #[test]
        #[should_panic(expected = "insufficient balance")]
        fn refuses_more_than_the_balance() {
            let _ = run(
                HOLDING,
                None,
                true,
                Some(&holding(DEFINITION, 100)),
                &message(101),
            );
        }

        #[test]
        #[should_panic(expected = "receiver is not a token holding")]
        fn fails_at_a_definition() {
            let _ = run(
                DEFINITION,
                None,
                true,
                Some(&definition(100, None)),
                &message(1),
            );
        }

        #[test]
        #[should_panic(expected = "receiver is not a token holding")]
        fn fails_at_an_empty_actor() {
            let _ = run(HOLDING, None, true, None, &message(1));
        }
    }

    mod increase_balance {
        use crate::test_utils::*;

        fn message(definition: AccountId, amount: u128) -> Internal {
            Internal::IncreaseBalance { definition, amount }
        }

        fn transferred(amount: u128) -> ProgramEvent {
            Transferred {
                definition: DEFINITION,
                from: HOLDING,
                to: RECIPIENT,
                amount,
            }
            .to_program_event()
        }

        #[test]
        fn credits_an_existing_holding() {
            let response = run(
                RECIPIENT,
                Some(token(HOLDING)),
                false,
                Some(&holding(DEFINITION, 5)),
                &message(DEFINITION, 40),
            );
            assert_eq!(
                response,
                write(&holding(DEFINITION, 45)).event(transferred(40))
            );
        }

        /// A mint: the definition already announced it as `Minted`.
        #[test]
        fn a_credit_from_the_definition_emits_no_transfer() {
            let response = run(
                RECIPIENT,
                Some(token(DEFINITION)),
                false,
                Some(&holding(DEFINITION, 5)),
                &message(DEFINITION, 40),
            );
            assert_eq!(response, write(&holding(DEFINITION, 45)));
        }

        #[test]
        #[should_panic(expected = "recipient has no token holding; initialize it first")]
        fn refuses_an_empty_recipient() {
            let _ = run(
                RECIPIENT,
                Some(token(HOLDING)),
                false,
                None,
                &message(DEFINITION, 40),
            );
        }

        #[test]
        #[should_panic(expected = "recipient holds another token")]
        fn refuses_a_holding_of_another_definition() {
            let _ = run(
                RECIPIENT,
                Some(token(HOLDING)),
                false,
                Some(&holding(OTHER_DEFINITION, 0)),
                &message(DEFINITION, 40),
            );
        }

        #[test]
        #[should_panic(expected = "recipient is a token definition")]
        fn refuses_a_definition() {
            let _ = run(
                OTHER_DEFINITION,
                Some(token(HOLDING)),
                false,
                Some(&definition(0, None)),
                &message(DEFINITION, 40),
            );
        }

        #[test]
        #[should_panic(expected = "balance overflow")]
        fn refuses_an_overflow() {
            let _ = run(
                RECIPIENT,
                Some(token(HOLDING)),
                false,
                Some(&holding(DEFINITION, u128::MAX)),
                &message(DEFINITION, 1),
            );
        }

        /// Supply conservation: outside programs can't get a credit decoded.
        #[test]
        fn is_never_decoded_from_an_outside_program() {
            let result = std::panic::catch_unwind(|| {
                run(
                    RECIPIENT,
                    Some(outsider(HOLDING)),
                    true,
                    Some(&holding(DEFINITION, 0)),
                    &message(DEFINITION, 40),
                )
            });
            if let Ok(response) = result {
                assert_ne!(
                    response.post_state,
                    Some(ActorState::from(holding(DEFINITION, 40).encode())),
                    "an outside program must not be able to credit a holding"
                );
            }
        }
    }
}
