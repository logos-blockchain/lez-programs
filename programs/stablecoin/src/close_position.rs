//! `ClosePosition`: the owner clears a settled position. Only the position is involved, so it
//! works while the protocol is frozen.

use lee_core::program::{ReceiveInput, Response};
use stablecoin_core::event::{PositionClosed, StablecoinEvent as _};

use crate::{checks, position_state};

/// At the owner's actor, signed as a root message: an empty actor state means "no position".
/// The vault's empty token holding stays; opening again initializes it idempotently.
///
/// # Panics
/// - `"only the account's owner may send this"` when the message is nested or unsigned.
/// - `"this account has no position"` when the receiver holds no position.
/// - `"Position still has debt"` / `"Position still holds collateral"` when it is not settled.
pub(crate) fn close_position(input: &ReceiveInput) -> Response {
    checks::assert_owner(input);
    let position = position_state(input);
    assert!(
        position.normalized_debt_amount == 0,
        "Position still has debt"
    );
    assert!(
        position.collateral_amount == 0,
        "Position still holds collateral"
    );
    Response::set_state(Vec::new()).event(
        PositionClosed {
            owner: input.receiver.account_id,
        }
        .to_program_event(),
    )
}

#[cfg(test)]
mod tests {
    use stablecoin_core::event::{PositionClosed, StablecoinEvent as _};

    use super::*;
    use crate::test_utils::*;

    fn position(collateral_amount: u128, normalized_debt_amount: u128) -> StablecoinState {
        StablecoinState::Position(Position {
            token_program_id: TOKEN,
            collateral_definition_id: COLLATERAL_DEFINITION,
            collateral_amount,
            normalized_debt_amount,
        })
    }

    fn close(from: Option<Actor>, is_authorized: bool, pre: Option<&StablecoinState>) -> Response {
        run(
            stablecoin(OWNER),
            from,
            is_authorized,
            pre,
            &Message::ClosePosition,
        )
    }

    #[test]
    fn closing_an_empty_position_clears_the_state() {
        assert_eq!(
            close(None, true, Some(&position(0, 0))),
            Response::set_state(Vec::new())
                .event(PositionClosed { owner: OWNER }.to_program_event())
        );
    }

    #[test]
    #[should_panic(expected = "Position still has debt")]
    fn closing_with_debt_panics() {
        let _ = close(None, true, Some(&position(0, 1)));
    }

    #[test]
    #[should_panic(expected = "Position still holds collateral")]
    fn closing_with_collateral_panics() {
        let _ = close(None, true, Some(&position(1, 0)));
    }

    #[test]
    #[should_panic(expected = "this account has no position")]
    fn closing_without_a_position_panics() {
        let _ = close(None, true, None);
    }

    #[test]
    #[should_panic(expected = "only the account's owner may send this")]
    fn closing_from_another_actor_panics() {
        let _ = close(Some(outsider(OTHER_PROGRAM)), true, Some(&position(0, 0)));
    }

    #[test]
    #[should_panic(expected = "only the account's owner may send this")]
    fn closing_without_the_owners_signature_panics() {
        let _ = close(None, false, Some(&position(0, 0)));
    }
}
