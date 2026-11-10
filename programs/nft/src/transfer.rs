//! `Transfer`: the sender gives up its whole value and credits the recipient asynchronously,
//! as in the token program: the credit carries no authorization.

use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response},
};
use nft_core::{
    event::{NftEvent as _, Transferred},
    Internal, NftCopy, NftMaster, NftState, NftValue,
};

use crate::{assert_holds_nothing_of, nft_actor};

/// At the sender's holding: a master moves its whole print balance, a copy itself.
pub(crate) fn transfer(input: &ReceiveInput, to: AccountId) -> Response {
    assert!(
        input.is_authorized,
        "sender holding must authorize the transfer"
    );
    let (definition, value, emptied) = match NftState::decode(&input.pre_state) {
        Some(NftState::Master(master)) => {
            assert!(master.print_balance > 0, "sender holds no NFT");
            let emptied = NftState::Master(NftMaster {
                print_balance: 0,
                ..master
            });
            (
                master.definition_id,
                NftValue::Master(master.print_balance),
                emptied,
            )
        }
        Some(NftState::Copy(copy)) => {
            assert!(copy.owned, "sender holds no NFT");
            let emptied = NftState::Copy(NftCopy {
                owned: false,
                ..copy
            });
            (copy.definition_id, NftValue::Copy, emptied)
        }
        Some(NftState::Definition(_)) | None => panic!("receiver is not an NFT holding"),
    };
    Response::set_state(emptied.encode()).send_async(
        nft_actor(input, to),
        &Internal::Receive { definition, value },
    )
}

/// At the recipient: a holding of the same definition that holds nothing takes the value.
pub(crate) fn receive(
    input: &ReceiveInput,
    from: Actor,
    definition: AccountId,
    value: NftValue,
) -> Response {
    assert_holds_nothing_of(input, definition);
    let state = match value {
        NftValue::Master(print_balance) => NftState::Master(NftMaster {
            definition_id: definition,
            print_balance,
        }),
        NftValue::Copy => NftState::Copy(NftCopy {
            definition_id: definition,
            owned: true,
        }),
    };
    Response::set_state(state.encode()).event(
        Transferred {
            definition,
            from: from.account_id,
            to: input.receiver.account_id,
            value,
        }
        .to_program_event(),
    )
}

#[cfg(test)]
mod tests {
    mod transfer {
        use crate::test_utils::*;

        fn message() -> Message {
            Message::Transfer { to: RECIPIENT }
        }

        fn receive(value: NftValue) -> Internal {
            Internal::Receive {
                definition: DEFINITION,
                value,
            }
        }

        /// Supply invariant: `Receive { Master(n) }` follows the master dropping from n to 0.
        #[test]
        fn a_master_moves_its_whole_balance() {
            let response = run(MASTER, None, true, Some(&master(DEFINITION, 7)), &message());
            let expected = write(&master(DEFINITION, 0))
                .send_async(nft(RECIPIENT), &receive(NftValue::Master(7)));
            assert_eq!(response, expected);
            assert!(
                response.sync_messages.is_empty(),
                "no grant may travel with the credit"
            );
        }

        /// Supply invariant: `Receive { Copy }` follows the copy clearing `owned`.
        #[test]
        fn a_copy_moves_itself() {
            let response = run(
                HOLDING,
                None,
                true,
                Some(&copy(DEFINITION, true)),
                &message(),
            );
            let expected = write(&copy(DEFINITION, false))
                .send_async(nft(RECIPIENT), &receive(NftValue::Copy));
            assert_eq!(response, expected);
        }

        #[test]
        #[should_panic(expected = "sender holding must authorize the transfer")]
        fn refuses_an_unauthorized_sender() {
            let _ = run(
                HOLDING,
                None,
                false,
                Some(&copy(DEFINITION, true)),
                &message(),
            );
        }

        #[test]
        #[should_panic(expected = "sender holds no NFT")]
        fn refuses_a_master_with_nothing_left() {
            let _ = run(MASTER, None, true, Some(&master(DEFINITION, 0)), &message());
        }

        #[test]
        #[should_panic(expected = "sender holds no NFT")]
        fn refuses_a_copy_it_does_not_own() {
            let _ = run(
                HOLDING,
                None,
                true,
                Some(&copy(DEFINITION, false)),
                &message(),
            );
        }

        #[test]
        #[should_panic(expected = "receiver is not an NFT holding")]
        fn fails_at_a_definition() {
            let _ = run(DEFINITION, None, true, Some(&definition(100)), &message());
        }

        #[test]
        #[should_panic(expected = "receiver is not an NFT holding")]
        fn fails_at_an_empty_actor() {
            let _ = run(HOLDING, None, true, None, &message());
        }
    }

    mod receive {
        use crate::test_utils::*;

        fn message(definition: AccountId, value: NftValue) -> Internal {
            Internal::Receive { definition, value }
        }

        fn transferred(value: NftValue) -> ProgramEvent {
            Transferred {
                definition: DEFINITION,
                from: HOLDING,
                to: RECIPIENT,
                value,
            }
            .to_program_event()
        }

        #[test]
        fn an_empty_copy_takes_a_copy() {
            let response = run(
                RECIPIENT,
                Some(nft(HOLDING)),
                false,
                Some(&copy(DEFINITION, false)),
                &message(DEFINITION, NftValue::Copy),
            );
            assert_eq!(
                response,
                write(&copy(DEFINITION, true)).event(transferred(NftValue::Copy))
            );
        }

        /// Initialization writes a copy, which can still become the master.
        #[test]
        fn an_empty_copy_takes_a_master() {
            let response = run(
                RECIPIENT,
                Some(nft(HOLDING)),
                false,
                Some(&copy(DEFINITION, false)),
                &message(DEFINITION, NftValue::Master(7)),
            );
            assert_eq!(
                response,
                write(&master(DEFINITION, 7)).event(transferred(NftValue::Master(7)))
            );
        }

        #[test]
        fn an_empty_master_takes_a_master() {
            let response = run(
                RECIPIENT,
                Some(nft(HOLDING)),
                false,
                Some(&master(DEFINITION, 0)),
                &message(DEFINITION, NftValue::Master(7)),
            );
            assert_eq!(
                response,
                write(&master(DEFINITION, 7)).event(transferred(NftValue::Master(7)))
            );
        }

        #[test]
        fn an_empty_master_takes_a_copy() {
            let response = run(
                RECIPIENT,
                Some(nft(HOLDING)),
                false,
                Some(&master(DEFINITION, 0)),
                &message(DEFINITION, NftValue::Copy),
            );
            assert_eq!(
                response,
                write(&copy(DEFINITION, true)).event(transferred(NftValue::Copy))
            );
        }

        #[test]
        #[should_panic(expected = "recipient already holds an NFT")]
        fn refuses_an_owned_copy() {
            let _ = run(
                RECIPIENT,
                Some(nft(HOLDING)),
                false,
                Some(&copy(DEFINITION, true)),
                &message(DEFINITION, NftValue::Copy),
            );
        }

        #[test]
        #[should_panic(expected = "recipient already holds an NFT")]
        fn refuses_a_master_with_a_balance() {
            let _ = run(
                RECIPIENT,
                Some(nft(HOLDING)),
                false,
                Some(&master(DEFINITION, 1)),
                &message(DEFINITION, NftValue::Master(7)),
            );
        }

        #[test]
        #[should_panic(expected = "recipient has no NFT holding; initialize it first")]
        fn refuses_an_empty_recipient() {
            let _ = run(
                RECIPIENT,
                Some(nft(HOLDING)),
                false,
                None,
                &message(DEFINITION, NftValue::Copy),
            );
        }

        #[test]
        #[should_panic(expected = "recipient holds another NFT")]
        fn refuses_a_holding_of_another_definition() {
            let _ = run(
                RECIPIENT,
                Some(nft(HOLDING)),
                false,
                Some(&copy(OTHER_DEFINITION, false)),
                &message(DEFINITION, NftValue::Copy),
            );
        }

        #[test]
        #[should_panic(expected = "recipient is an NFT definition")]
        fn refuses_a_definition() {
            let _ = run(
                OTHER_DEFINITION,
                Some(nft(HOLDING)),
                false,
                Some(&definition(1)),
                &message(DEFINITION, NftValue::Copy),
            );
        }

        /// Supply invariant: outside programs can't get a credit decoded.
        #[test]
        fn is_never_decoded_from_an_outside_program() {
            let result = std::panic::catch_unwind(|| {
                run(
                    RECIPIENT,
                    Some(outsider(HOLDING)),
                    true,
                    Some(&copy(DEFINITION, false)),
                    &message(DEFINITION, NftValue::Copy),
                )
            });
            if let Ok(response) = result {
                assert_ne!(
                    response.post_state,
                    Some(ActorState::from(copy(DEFINITION, true).encode())),
                    "an outside program must not be able to credit a holding"
                );
            }
        }
    }
}
