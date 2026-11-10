//! `NewDefinition`: the definition writes itself and initializes the master with its supply.

use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response},
};
use nft_core::{
    event::{NewDefinition, NftEvent as _},
    Internal, NftDefinition, NftMaster, NftMetadata, NftState, MAX_CREATORS_LEN, MAX_NAME_LEN,
    MAX_URI_LEN,
};

use crate::nft_actor;

/// At the empty definition. Any sender, as in the token program.
pub(crate) fn new_definition(
    input: &ReceiveInput,
    name: String,
    printable_supply: u128,
    metadata: NftMetadata,
    master: AccountId,
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
        "nft name must be 1 to {MAX_NAME_LEN} bytes"
    );
    assert!(
        metadata.uri.len() <= MAX_URI_LEN,
        "metadata uri must be at most {MAX_URI_LEN} bytes"
    );
    assert!(
        metadata.creators.len() <= MAX_CREATORS_LEN,
        "metadata creators must be at most {MAX_CREATORS_LEN} bytes"
    );
    assert!(printable_supply >= 1, "printable supply must be at least 1");
    let definition = input.receiver.account_id;
    assert!(
        master != definition,
        "the master must be another account than the definition"
    );

    let event = NewDefinition {
        definition,
        master,
        name: name.clone(),
        printable_supply,
    };
    let state = NftState::Definition(NftDefinition {
        name,
        printable_supply,
        metadata,
    });
    Response::set_state(state.encode())
        .send_sync(
            nft_actor(input, master),
            &Internal::InitMaster {
                definition,
                print_balance: printable_supply,
            },
        )
        .event(event.to_program_event())
}

/// At the empty master, from its definition.
pub(crate) fn init_master(
    input: &ReceiveInput,
    from: Actor,
    definition: AccountId,
    print_balance: u128,
) -> Response {
    assert!(input.pre_state.is_empty(), "master account must be empty");
    assert!(
        input.is_authorized,
        "master account must authorize its creation"
    );
    assert!(
        from.account_id == definition,
        "only the definition initializes its master"
    );
    Response::set_state(
        NftState::Master(NftMaster {
            definition_id: definition,
            print_balance,
        })
        .encode(),
    )
}

#[cfg(test)]
mod tests {
    mod new_definition {
        use crate::test_utils::*;

        fn message_with(name: &str, printable_supply: u128, metadata: NftMetadata) -> Message {
            Message::NewDefinition {
                name: name.to_owned(),
                printable_supply,
                metadata,
                master: MASTER,
            }
        }

        fn message(name: &str) -> Message {
            message_with(name, 100, metadata())
        }

        #[test]
        fn writes_the_definition_and_initializes_the_master_with_the_supply() {
            let response = run(DEFINITION, None, true, None, &message("Art"));
            let expected = write(&definition(100))
                .send_sync(
                    nft(MASTER),
                    &Internal::InitMaster {
                        definition: DEFINITION,
                        print_balance: 100,
                    },
                )
                .event(
                    NewDefinition {
                        definition: DEFINITION,
                        master: MASTER,
                        name: "Art".to_owned(),
                        printable_supply: 100,
                    }
                    .to_program_event(),
                );
            assert_eq!(response, expected);
        }

        #[test]
        fn may_be_sent_by_another_program() {
            let response = run(
                DEFINITION,
                Some(outsider(HOLDING)),
                true,
                None,
                &message("Art"),
            );
            assert_eq!(
                response.post_state,
                Some(ActorState::from(definition(100).encode()))
            );
        }

        #[test]
        fn accepts_a_single_unit() {
            let response = run(
                DEFINITION,
                None,
                true,
                None,
                &message_with("Art", 1, metadata()),
            );
            assert!(response.post_state.is_some());
        }

        #[test]
        fn accepts_strings_of_the_maximum_length() {
            let name = "a".repeat(MAX_NAME_LEN);
            let metadata = NftMetadata {
                standard: MetadataStandard::Expanded,
                uri: "u".repeat(MAX_URI_LEN),
                creators: "c".repeat(MAX_CREATORS_LEN),
            };
            let response = run(
                DEFINITION,
                None,
                true,
                None,
                &message_with(&name, 100, metadata),
            );
            assert!(response.post_state.is_some());
        }

        #[test]
        #[should_panic(expected = "nft name must be 1 to 64 bytes")]
        fn refuses_an_empty_name() {
            let _ = run(DEFINITION, None, true, None, &message(""));
        }

        #[test]
        #[should_panic(expected = "nft name must be 1 to 64 bytes")]
        fn refuses_a_name_one_byte_too_long() {
            let _ = run(
                DEFINITION,
                None,
                true,
                None,
                &message(&"a".repeat(MAX_NAME_LEN + 1)),
            );
        }

        #[test]
        #[should_panic(expected = "metadata uri must be at most 256 bytes")]
        fn refuses_a_uri_one_byte_too_long() {
            let metadata = NftMetadata {
                uri: "u".repeat(MAX_URI_LEN + 1),
                ..metadata()
            };
            let _ = run(
                DEFINITION,
                None,
                true,
                None,
                &message_with("Art", 100, metadata),
            );
        }

        #[test]
        #[should_panic(expected = "metadata creators must be at most 256 bytes")]
        fn refuses_creators_one_byte_too_long() {
            let metadata = NftMetadata {
                creators: "c".repeat(MAX_CREATORS_LEN + 1),
                ..metadata()
            };
            let _ = run(
                DEFINITION,
                None,
                true,
                None,
                &message_with("Art", 100, metadata),
            );
        }

        #[test]
        #[should_panic(expected = "printable supply must be at least 1")]
        fn refuses_a_zero_supply() {
            let _ = run(
                DEFINITION,
                None,
                true,
                None,
                &message_with("Art", 0, metadata()),
            );
        }

        #[test]
        #[should_panic(expected = "definition account must authorize its creation")]
        fn refuses_an_unauthorized_definition() {
            let _ = run(DEFINITION, None, false, None, &message("Art"));
        }

        #[test]
        #[should_panic(expected = "definition account must be empty")]
        fn refuses_an_existing_definition() {
            let _ = run(
                DEFINITION,
                None,
                true,
                Some(&definition(1)),
                &message("Art"),
            );
        }

        #[test]
        #[should_panic(expected = "definition account must be empty")]
        fn refuses_an_existing_holding() {
            let _ = run(
                DEFINITION,
                None,
                true,
                Some(&copy(OTHER_DEFINITION, false)),
                &message("Art"),
            );
        }

        #[test]
        #[should_panic(expected = "the master must be another account than the definition")]
        fn refuses_the_definition_as_its_own_master() {
            let _ = run(
                DEFINITION,
                None,
                true,
                None,
                &Message::NewDefinition {
                    name: "Art".to_owned(),
                    printable_supply: 100,
                    metadata: metadata(),
                    master: DEFINITION,
                },
            );
        }
    }

    mod init_master {
        use crate::test_utils::*;

        fn message() -> Internal {
            Internal::InitMaster {
                definition: DEFINITION,
                print_balance: 100,
            }
        }

        /// Supply invariant: the master starts with the whole supply.
        #[test]
        fn writes_the_master() {
            let response = run(MASTER, Some(nft(DEFINITION)), true, None, &message());
            assert_eq!(response, write(&master(DEFINITION, 100)));
        }

        #[test]
        #[should_panic(expected = "master account must be empty")]
        fn refuses_an_existing_holding() {
            let _ = run(
                MASTER,
                Some(nft(DEFINITION)),
                true,
                Some(&copy(DEFINITION, false)),
                &message(),
            );
        }

        #[test]
        #[should_panic(expected = "master account must authorize its creation")]
        fn refuses_an_unauthorized_master() {
            let _ = run(MASTER, Some(nft(DEFINITION)), false, None, &message());
        }

        #[test]
        #[should_panic(expected = "only the definition initializes its master")]
        fn refuses_another_nft_actor() {
            let _ = run(MASTER, Some(nft(HOLDING)), true, None, &message());
        }
    }
}
