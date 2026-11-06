//! `SetAuthority`: rotate or renounce the mint authority, routed like `Mint`.

use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response},
};
use token_core::{
    event::{AuthorityChanged, TokenEvent as _},
    Internal, TokenDefinition, TokenState,
};

use crate::{assert_authority, assert_not_zero, definition_state, forward_as_authority};

/// At the definition (self authority) or at the authority's actor.
pub(crate) fn set_authority(
    input: &ReceiveInput,
    definition: AccountId,
    new_authority: Option<AccountId>,
) -> Response {
    assert_not_zero(new_authority);
    if definition == input.receiver.account_id {
        assert!(
            input.is_authorized,
            "definition must authorize changing its own authority"
        );
        let state = definition_state(input);
        assert_authority(&state, definition);
        replace_authority(input, state, new_authority)
    } else {
        forward_as_authority(
            input,
            definition,
            &Internal::ChangeAuthority { new_authority },
        )
    }
}

/// At the definition, from its authority's actor.
pub(crate) fn change_authority(
    input: &ReceiveInput,
    from: Actor,
    new_authority: Option<AccountId>,
) -> Response {
    assert_not_zero(new_authority);
    let definition = definition_state(input);
    assert_authority(&definition, from.account_id);
    replace_authority(input, definition, new_authority)
}

fn replace_authority(
    input: &ReceiveInput,
    mut definition: TokenDefinition,
    new_authority: Option<AccountId>,
) -> Response {
    let old = definition.authority;
    definition.authority = new_authority;
    Response::set_state(TokenState::Definition(definition).encode()).event(
        AuthorityChanged {
            definition: input.receiver.account_id,
            old,
            new: new_authority,
        }
        .to_program_event(),
    )
}

#[cfg(test)]
mod tests {
    use crate::test_utils::*;

    fn message(new_authority: Option<AccountId>) -> Message {
        Message::SetAuthority {
            definition: DEFINITION,
            new_authority,
        }
    }

    fn changed(old: Option<AccountId>, new: Option<AccountId>) -> ProgramEvent {
        AuthorityChanged {
            definition: DEFINITION,
            old,
            new,
        }
        .to_program_event()
    }

    #[test]
    fn a_self_authority_definition_rotates() {
        let response = run(
            DEFINITION,
            None,
            true,
            Some(&definition(1, Some(DEFINITION))),
            &message(Some(NEW_AUTHORITY)),
        );
        assert_eq!(
            response,
            write(&definition(1, Some(NEW_AUTHORITY)))
                .event(changed(Some(DEFINITION), Some(NEW_AUTHORITY)))
        );
    }

    #[test]
    fn a_self_authority_definition_renounces() {
        let response = run(
            DEFINITION,
            None,
            true,
            Some(&definition(1, Some(DEFINITION))),
            &message(None),
        );
        assert_eq!(
            response,
            write(&definition(1, None)).event(changed(Some(DEFINITION), None))
        );
    }

    #[test]
    #[should_panic(expected = "definition must authorize changing its own authority")]
    fn a_self_authority_definition_must_be_authorized() {
        let _ = run(
            DEFINITION,
            None,
            false,
            Some(&definition(1, Some(DEFINITION))),
            &message(None),
        );
    }

    #[test]
    #[should_panic(expected = "mint authority is renounced")]
    fn a_renounced_authority_is_final() {
        let _ = run(
            DEFINITION,
            None,
            true,
            Some(&definition(1, None)),
            &message(Some(DEFINITION)),
        );
    }

    #[test]
    #[should_panic(expected = "mint authority must not be the zero account id")]
    fn refuses_the_zero_account() {
        let _ = run(
            DEFINITION,
            None,
            true,
            Some(&definition(1, Some(DEFINITION))),
            &message(Some(ZERO)),
        );
    }

    #[test]
    fn the_authority_actor_forwards() {
        let response = run(AUTHORITY, None, true, None, &message(Some(NEW_AUTHORITY)));
        assert_eq!(
            response,
            Response::keep_state().send_sync(
                token(DEFINITION),
                &Internal::ChangeAuthority {
                    new_authority: Some(NEW_AUTHORITY),
                }
            )
        );
    }

    #[test]
    #[should_panic(expected = "mint authority must authorize")]
    fn the_authority_actor_must_be_authorized() {
        let _ = run(AUTHORITY, None, false, None, &message(Some(NEW_AUTHORITY)));
    }

    #[test]
    fn the_definition_rotates_for_its_authority() {
        let response = run(
            DEFINITION,
            Some(token(AUTHORITY)),
            false,
            Some(&definition(1, Some(AUTHORITY))),
            &Internal::ChangeAuthority {
                new_authority: Some(NEW_AUTHORITY),
            },
        );
        assert_eq!(
            response,
            write(&definition(1, Some(NEW_AUTHORITY)))
                .event(changed(Some(AUTHORITY), Some(NEW_AUTHORITY)))
        );
    }

    #[test]
    #[should_panic(expected = "sender is not the mint authority")]
    fn the_old_authority_loses_its_rights() {
        let _ = run(
            DEFINITION,
            Some(token(AUTHORITY)),
            false,
            Some(&definition(1, Some(NEW_AUTHORITY))),
            &Internal::IncreaseSupply {
                to: RECIPIENT,
                amount: 1,
            },
        );
    }

    #[test]
    #[should_panic(expected = "mint authority must not be the zero account id")]
    fn the_definition_refuses_the_zero_account() {
        let _ = run(
            DEFINITION,
            Some(token(AUTHORITY)),
            false,
            Some(&definition(1, Some(AUTHORITY))),
            &Internal::ChangeAuthority {
                new_authority: Some(ZERO),
            },
        );
    }
}
