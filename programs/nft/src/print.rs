//! `PrintNft`: the master lowers its print balance by one and credits a new copy to an
//! initialized holding, like a token mint (`nft.md` §2.3). The recipient doesn't sign.

use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response},
};
use nft_core::{
    event::{NftEvent as _, Printed},
    Internal, NftCopy, NftState,
};

use crate::{assert_holds_nothing_of, nft_actor};

/// At the master. Its own unit is not printable, so it prints while above 1.
pub(crate) fn print_nft(input: &ReceiveInput, to: AccountId) -> Response {
    assert!(input.is_authorized, "master must authorize the print");
    let mut master = match NftState::decode(&input.pre_state) {
        Some(NftState::Master(master)) => master,
        Some(NftState::Copy(_) | NftState::Definition(_)) | None => {
            panic!("receiver is not an NFT master")
        }
    };
    assert!(master.print_balance > 1, "insufficient print balance");
    master.print_balance = master
        .print_balance
        .checked_sub(1)
        .expect("print balance is above 1");
    let definition = master.definition_id;
    Response::set_state(NftState::Master(master).encode())
        .send_async(nft_actor(input, to), &Internal::ReceivePrint { definition })
}

/// At the recipient: a holding of the same definition that holds nothing becomes an owned
/// copy.
pub(crate) fn receive_print(input: &ReceiveInput, from: Actor, definition: AccountId) -> Response {
    assert_holds_nothing_of(input, definition);
    Response::set_state(
        NftState::Copy(NftCopy {
            definition_id: definition,
            owned: true,
        })
        .encode(),
    )
    .event(
        Printed {
            definition,
            master: from.account_id,
            to: input.receiver.account_id,
        }
        .to_program_event(),
    )
}

#[cfg(test)]
mod tests {
    mod print_nft {
        use crate::test_utils::*;

        fn message() -> Message {
            Message::PrintNft { to: RECIPIENT }
        }

        /// Supply invariant: `ReceivePrint` follows the master's balance dropping by one.
        #[test]
        fn lowers_the_balance_and_credits_a_copy_async() {
            let response = run(
                MASTER,
                None,
                true,
                Some(&master(DEFINITION, 10)),
                &message(),
            );
            let expected = write(&master(DEFINITION, 9)).send_async(
                nft(RECIPIENT),
                &Internal::ReceivePrint {
                    definition: DEFINITION,
                },
            );
            assert_eq!(response, expected);
            assert!(
                response.sync_messages.is_empty(),
                "no grant may travel with the credit"
            );
        }

        #[test]
        fn prints_down_to_the_masters_own_unit() {
            let response = run(MASTER, None, true, Some(&master(DEFINITION, 2)), &message());
            assert_eq!(
                response.post_state,
                Some(ActorState::from(master(DEFINITION, 1).encode()))
            );
        }

        #[test]
        #[should_panic(expected = "master must authorize the print")]
        fn refuses_an_unauthorized_master() {
            let _ = run(
                MASTER,
                None,
                false,
                Some(&master(DEFINITION, 10)),
                &message(),
            );
        }

        #[test]
        #[should_panic(expected = "insufficient print balance")]
        fn refuses_the_masters_own_unit() {
            let _ = run(MASTER, None, true, Some(&master(DEFINITION, 1)), &message());
        }

        #[test]
        #[should_panic(expected = "receiver is not an NFT master")]
        fn fails_at_a_copy() {
            let _ = run(
                HOLDING,
                None,
                true,
                Some(&copy(DEFINITION, true)),
                &message(),
            );
        }

        #[test]
        #[should_panic(expected = "receiver is not an NFT master")]
        fn fails_at_a_definition() {
            let _ = run(DEFINITION, None, true, Some(&definition(10)), &message());
        }
    }

    mod receive_print {
        use crate::test_utils::*;

        fn message(definition: AccountId) -> Internal {
            Internal::ReceivePrint { definition }
        }

        fn printed() -> ProgramEvent {
            Printed {
                definition: DEFINITION,
                master: MASTER,
                to: RECIPIENT,
            }
            .to_program_event()
        }

        #[test]
        fn an_empty_copy_becomes_an_owned_copy() {
            let response = run(
                RECIPIENT,
                Some(nft(MASTER)),
                false,
                Some(&copy(DEFINITION, false)),
                &message(DEFINITION),
            );
            assert_eq!(response, write(&copy(DEFINITION, true)).event(printed()));
        }

        #[test]
        fn an_empty_master_becomes_an_owned_copy() {
            let response = run(
                RECIPIENT,
                Some(nft(MASTER)),
                false,
                Some(&master(DEFINITION, 0)),
                &message(DEFINITION),
            );
            assert_eq!(response, write(&copy(DEFINITION, true)).event(printed()));
        }

        #[test]
        #[should_panic(expected = "recipient already holds an NFT")]
        fn refuses_an_owned_copy() {
            let _ = run(
                RECIPIENT,
                Some(nft(MASTER)),
                false,
                Some(&copy(DEFINITION, true)),
                &message(DEFINITION),
            );
        }

        #[test]
        #[should_panic(expected = "recipient has no NFT holding; initialize it first")]
        fn refuses_an_empty_recipient() {
            let _ = run(
                RECIPIENT,
                Some(nft(MASTER)),
                false,
                None,
                &message(DEFINITION),
            );
        }

        #[test]
        #[should_panic(expected = "recipient holds another NFT")]
        fn refuses_a_holding_of_another_definition() {
            let _ = run(
                RECIPIENT,
                Some(nft(MASTER)),
                false,
                Some(&copy(OTHER_DEFINITION, false)),
                &message(DEFINITION),
            );
        }

        /// Supply invariant: outside programs can't get a print decoded.
        #[test]
        fn is_never_decoded_from_an_outside_program() {
            let result = std::panic::catch_unwind(|| {
                run(
                    RECIPIENT,
                    Some(outsider(MASTER)),
                    true,
                    Some(&copy(DEFINITION, false)),
                    &message(DEFINITION),
                )
            });
            if let Ok(response) = result {
                assert_ne!(
                    response.post_state,
                    Some(ActorState::from(copy(DEFINITION, true).encode())),
                    "an outside program must not be able to print"
                );
            }
        }
    }
}
