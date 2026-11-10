//! `InitializeHolding`: holding → definition → holding, as in the token program
//! (`docs/actor-model/initialize-holding.md`), so a holding only ever exists for a definition
//! that does.
//!
//! The holding is written as a copy that holds nothing; it can still receive a master
//! (`nft.md` §2.2).

use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response},
};
use nft_core::{Internal, NftCopy, NftMaster, NftState};

use crate::{definition_state, nft_actor};

/// At the holding: ask `definition` to initialize it.
pub(crate) fn initialize_holding(input: &ReceiveInput, definition: AccountId) -> Response {
    assert!(
        input.is_authorized,
        "holding account must authorize its initialization"
    );
    match NftState::decode(&input.pre_state) {
        None => Response::keep_state()
            .send_sync(nft_actor(input, definition), &Internal::RequestHolding),
        // Idempotent, so ATA `Create` can be repeated.
        Some(NftState::Master(NftMaster { definition_id, .. }))
        | Some(NftState::Copy(NftCopy { definition_id, .. }))
            if definition_id == definition =>
        {
            Response::keep_state()
        }
        Some(NftState::Master(_) | NftState::Copy(_) | NftState::Definition(_)) => {
            panic!("account already holds another NFT state")
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
        },
    )
}

/// At the holding, from its definition: write a holding that holds nothing.
pub(crate) fn init_holding(input: &ReceiveInput, from: Actor, definition: AccountId) -> Response {
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
        NftState::Copy(NftCopy {
            definition_id: definition,
            owned: false,
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
                Response::keep_state().send_sync(nft(DEFINITION), &Internal::RequestHolding)
            );
        }

        /// ATA forwards with its seed: the NFT program only sees `is_authorized`.
        #[test]
        fn may_be_sent_by_another_program() {
            let response = run(
                HOLDING,
                Some(outsider(HOLDING)),
                true,
                None,
                &message(DEFINITION),
            );
            assert_eq!(
                response,
                Response::keep_state().send_sync(nft(DEFINITION), &Internal::RequestHolding)
            );
        }

        #[test]
        fn is_a_no_op_on_a_copy_of_the_same_definition() {
            let response = run(
                HOLDING,
                None,
                true,
                Some(&copy(DEFINITION, true)),
                &message(DEFINITION),
            );
            assert_eq!(response, Response::keep_state());
        }

        #[test]
        fn is_a_no_op_on_a_master_of_the_same_definition() {
            let response = run(
                MASTER,
                None,
                true,
                Some(&master(DEFINITION, 10)),
                &message(DEFINITION),
            );
            assert_eq!(response, Response::keep_state());
        }

        #[test]
        #[should_panic(expected = "account already holds another NFT state")]
        fn refuses_a_copy_of_another_definition() {
            let _ = run(
                HOLDING,
                None,
                true,
                Some(&copy(OTHER_DEFINITION, false)),
                &message(DEFINITION),
            );
        }

        #[test]
        #[should_panic(expected = "account already holds another NFT state")]
        fn refuses_a_master_of_another_definition() {
            let _ = run(
                HOLDING,
                None,
                true,
                Some(&master(OTHER_DEFINITION, 0)),
                &message(DEFINITION),
            );
        }

        #[test]
        #[should_panic(expected = "account already holds another NFT state")]
        fn refuses_a_definition() {
            let _ = run(
                HOLDING,
                None,
                true,
                Some(&definition(1)),
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
        fn the_definition_replies_to_the_sender() {
            let response = run(
                DEFINITION,
                Some(nft(HOLDING)),
                false,
                Some(&definition(100)),
                &Internal::RequestHolding,
            );
            assert_eq!(
                response,
                Response::keep_state().send_sync(
                    nft(HOLDING),
                    &Internal::InitHolding {
                        definition: DEFINITION,
                    }
                )
            );
        }

        #[test]
        #[should_panic(expected = "receiver is not an NFT definition")]
        fn fails_at_a_master() {
            let _ = run(
                MASTER,
                Some(nft(HOLDING)),
                false,
                Some(&master(DEFINITION, 10)),
                &Internal::RequestHolding,
            );
        }

        #[test]
        #[should_panic(expected = "receiver is not an NFT definition")]
        fn fails_at_an_empty_actor() {
            let _ = run(
                OTHER_DEFINITION,
                Some(nft(HOLDING)),
                false,
                None,
                &Internal::RequestHolding,
            );
        }
    }

    mod init_holding {
        use crate::test_utils::*;

        fn message() -> Internal {
            Internal::InitHolding {
                definition: DEFINITION,
            }
        }

        /// Supply invariant: a new holding holds nothing.
        #[test]
        fn writes_a_copy_that_holds_nothing() {
            let response = run(HOLDING, Some(nft(DEFINITION)), true, None, &message());
            assert_eq!(response, write(&copy(DEFINITION, false)));
        }

        #[test]
        #[should_panic(expected = "holding account must be empty")]
        fn refuses_an_existing_holding() {
            let _ = run(
                HOLDING,
                Some(nft(DEFINITION)),
                true,
                Some(&copy(DEFINITION, true)),
                &message(),
            );
        }

        #[test]
        #[should_panic(expected = "holding account must authorize its initialization")]
        fn refuses_an_unauthorized_holding() {
            let _ = run(HOLDING, Some(nft(DEFINITION)), false, None, &message());
        }

        #[test]
        #[should_panic(expected = "only the definition initializes its holdings")]
        fn refuses_another_nft_actor() {
            let _ = run(HOLDING, Some(nft(RECIPIENT)), true, None, &message());
        }
    }
}
