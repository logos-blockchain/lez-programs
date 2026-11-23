//! `OpenPosition`: a position is the stablecoin program's actor state on the owner's own
//! account. Opening asks the engine for approval (the protocol must not be frozen; the engine
//! also names the token program and the collateral), then initializes the vault's token holding
//! and pulls the initial collateral in.

use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response, SyncMessage},
};
use stablecoin_core::{
    compute_position_vault_pda, compute_position_vault_pda_seed,
    event::{PositionOpened, StablecoinEvent as _},
    Internal, Position, StablecoinState,
};

use crate::{checks, engine_actor, engine_state, program_id, save_position};

/// At the owner's actor, signed as a root message: ask the engine to approve a new position.
///
/// # Panics
/// - `"this actor is the engine"` when the receiver holds engine state.
/// - `"this account already has a position"` when it holds a position.
/// - `"only the account's owner may send this"` when the message is nested or unsigned.
pub(crate) fn open_position(
    input: &ReceiveInput,
    collateral_holding: AccountId,
    initial_collateral_amount: u128,
) -> Response {
    assert_no_state(input);
    checks::assert_owner(input);
    Response::keep_state().send_sync(
        engine_actor(input),
        &Internal::OpenRequest {
            collateral_holding,
            initial_collateral_amount,
        },
    )
}

/// At the engine, from the position's actor: approve unless the protocol is frozen, replying
/// to the sender with the token program and the collateral definition. Any actor of this
/// program may ask: only the program's own code, after its owner check, sends `OpenRequest`.
///
/// # Panics
/// - `"this actor is not the engine"` when the receiver holds no engine state.
/// - `"Protocol is frozen"` while the protocol is frozen.
pub(crate) fn approve(
    input: &ReceiveInput,
    from: Actor,
    collateral_holding: AccountId,
    initial_collateral_amount: u128,
) -> Response {
    let parameters = engine_state(input).parameters;
    assert!(!parameters.is_frozen, "Protocol is frozen");
    Response::keep_state().send_sync(
        from,
        &Internal::OpenApproved {
            token_program_id: parameters.token_program_id,
            collateral_definition_id: parameters.collateral_definition_id,
            collateral_holding,
            initial_collateral_amount,
        },
    )
}

/// At the owner's actor, from the engine: write the position, initialize the vault's token
/// holding and pull `initial_collateral_amount` in (no transfer when it is zero).
///
/// # Panics
/// - `"this actor is the engine"` / `"this account already has a position"` when the receiver holds
///   state.
/// - `"only the engine may send this"` when the sender is anyone else.
pub(crate) fn opened(
    input: &ReceiveInput,
    token_program_id: AccountId,
    collateral_definition_id: AccountId,
    collateral_holding: AccountId,
    initial_collateral_amount: u128,
) -> Response {
    assert_no_state(input);
    checks::assert_from_engine(input);
    let owner = input.receiver.account_id;
    let vault = compute_position_vault_pda(program_id(input), owner);
    let mut response = save_position(Position {
        token_program_id,
        collateral_amount: initial_collateral_amount,
        normalized_debt_amount: 0,
    })
    .send(
        SyncMessage::new(
            Actor::new(vault, token_program_id),
            &token_core::Message::InitializeHolding {
                definition: collateral_definition_id,
            },
        )
        .with_pda_seeds([compute_position_vault_pda_seed(owner)]),
    );
    if initial_collateral_amount > 0 {
        response = response.send_sync(
            Actor::new(collateral_holding, token_program_id),
            &token_core::Message::Transfer {
                to: vault,
                amount: initial_collateral_amount,
            },
        );
    }
    response.event(
        PositionOpened {
            owner,
            vault,
            collateral_amount: initial_collateral_amount,
        }
        .to_program_event(),
    )
}

/// The receiver has no position yet.
fn assert_no_state(input: &ReceiveInput) {
    match StablecoinState::decode(&input.pre_state) {
        Some(StablecoinState::Engine(_)) => panic!("this actor is the engine"),
        Some(StablecoinState::Position(_)) => panic!("this account already has a position"),
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use stablecoin_core::Internal;

    use super::*;
    use crate::test_utils::*;

    fn engine_state_at_rest() -> StablecoinState {
        StablecoinState::Engine(engine_at(1_000))
    }

    fn position_state_at_rest() -> StablecoinState {
        StablecoinState::Position(Position {
            token_program_id: TOKEN,
            collateral_amount: 1,
            normalized_debt_amount: 0,
        })
    }

    fn open(from: Option<Actor>, is_authorized: bool, pre: Option<&StablecoinState>) -> Response {
        run(
            stablecoin(OWNER),
            from,
            is_authorized,
            pre,
            &Message::OpenPosition {
                collateral_holding: HOLDING,
                initial_collateral_amount: 500,
            },
        )
    }

    fn request(engine_state: &Engine) -> Response {
        run(
            engine(),
            Some(stablecoin(OWNER)),
            false,
            Some(&StablecoinState::Engine(engine_state.clone())),
            &Internal::OpenRequest {
                collateral_holding: HOLDING,
                initial_collateral_amount: 500,
            },
        )
    }

    fn approved(
        initial_collateral_amount: u128,
        from: Actor,
        pre: Option<&StablecoinState>,
    ) -> Response {
        run(
            stablecoin(OWNER),
            Some(from),
            false,
            pre,
            &Internal::OpenApproved {
                token_program_id: TOKEN,
                collateral_definition_id: COLLATERAL_DEFINITION,
                collateral_holding: HOLDING,
                initial_collateral_amount,
            },
        )
    }

    fn initialize_vault() -> SyncMessage {
        SyncMessage::new(
            token(vault_id(OWNER)),
            &token_core::Message::InitializeHolding {
                definition: COLLATERAL_DEFINITION,
            },
        )
        .with_pda_seeds([compute_position_vault_pda_seed(OWNER)])
    }

    fn opened_position(collateral_amount: u128) -> Response {
        Response::set_state(
            StablecoinState::Position(Position {
                token_program_id: TOKEN,
                collateral_amount,
                normalized_debt_amount: 0,
            })
            .encode(),
        )
    }

    fn opened_event(collateral_amount: u128) -> lee_core::program::ProgramEvent {
        PositionOpened {
            owner: OWNER,
            vault: vault_id(OWNER),
            collateral_amount,
        }
        .to_program_event()
    }

    #[test]
    fn the_owner_asks_the_engine_for_approval() {
        assert_eq!(
            open(None, true, None),
            Response::keep_state().send_sync(
                engine(),
                &Internal::OpenRequest {
                    collateral_holding: HOLDING,
                    initial_collateral_amount: 500,
                },
            )
        );
    }

    #[test]
    #[should_panic(expected = "this account already has a position")]
    fn opening_over_an_existing_position_panics() {
        let _ = open(None, true, Some(&position_state_at_rest()));
    }

    #[test]
    #[should_panic(expected = "this actor is the engine")]
    fn opening_at_the_engine_panics() {
        let _ = open(None, true, Some(&engine_state_at_rest()));
    }

    #[test]
    #[should_panic(expected = "only the account's owner may send this")]
    fn opening_from_another_actor_panics() {
        let _ = open(Some(outsider(OTHER_PROGRAM)), true, None);
    }

    #[test]
    #[should_panic(expected = "only the account's owner may send this")]
    fn opening_without_the_owners_signature_panics() {
        let _ = open(None, false, None);
    }

    #[test]
    fn the_engine_approves_and_names_the_token_program_and_collateral() {
        assert_eq!(
            request(&engine_at(1_000)),
            Response::keep_state().send_sync(
                stablecoin(OWNER),
                &Internal::OpenApproved {
                    token_program_id: TOKEN,
                    collateral_definition_id: COLLATERAL_DEFINITION,
                    collateral_holding: HOLDING,
                    initial_collateral_amount: 500,
                },
            )
        );
    }

    #[test]
    #[should_panic(expected = "Protocol is frozen")]
    fn the_engine_refuses_while_frozen() {
        let mut frozen = engine_at(1_000);
        frozen.parameters.is_frozen = true;
        let _ = request(&frozen);
    }

    #[test]
    fn the_position_opens_and_pulls_the_collateral_in() {
        assert_eq!(
            approved(500, engine(), None),
            opened_position(500)
                .send(initialize_vault())
                .send_sync(
                    token(HOLDING),
                    &token_core::Message::Transfer {
                        to: vault_id(OWNER),
                        amount: 500,
                    },
                )
                .event(opened_event(500))
        );
    }

    #[test]
    fn opening_with_no_collateral_only_initializes_the_vault() {
        assert_eq!(
            approved(0, engine(), None),
            opened_position(0)
                .send(initialize_vault())
                .event(opened_event(0))
        );
    }

    #[test]
    #[should_panic(expected = "only the engine may send this")]
    fn approval_from_another_stablecoin_actor_panics() {
        let _ = approved(500, stablecoin(ADMIN), None);
    }

    #[test]
    #[should_panic(expected = "this account already has a position")]
    fn approval_over_an_existing_position_panics() {
        let _ = approved(500, engine(), Some(&position_state_at_rest()));
    }
}
