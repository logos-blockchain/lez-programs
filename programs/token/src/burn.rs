//! `Burn`: the holding debits itself and tells its definition to reduce the supply.

use lee_core::{
    account::Actor,
    program::{ReceiveInput, Response},
};
use token_core::{
    event::{Burned, TokenEvent as _},
    Internal, TokenState,
};

use crate::{definition_state, holding_state, token_actor};

/// At the holding.
pub(crate) fn burn(input: &ReceiveInput, amount: u128) -> Response {
    assert!(input.is_authorized, "holding must authorize the burn");
    let mut holding = holding_state(input);
    holding.balance = holding
        .balance
        .checked_sub(amount)
        .expect("insufficient balance");
    let definition = holding.definition_id;
    Response::set_state(TokenState::Holding(holding).encode()).send_sync(
        token_actor(input, definition),
        &Internal::DecreaseSupply { amount },
    )
}

/// At the holding's definition.
pub(crate) fn decrease_supply(input: &ReceiveInput, from: Actor, amount: u128) -> Response {
    let mut definition = definition_state(input);
    definition.total_supply = definition
        .total_supply
        .checked_sub(amount)
        .expect("burn exceeds total supply");
    let event = Burned {
        definition: input.receiver.account_id,
        holding: from.account_id,
        amount,
    };
    Response::set_state(TokenState::Definition(definition).encode()).event(event.to_program_event())
}

#[cfg(test)]
mod tests {
    use crate::test_utils::*;

    #[test]
    fn debits_and_tells_the_definition() {
        let response = run(
            HOLDING,
            None,
            true,
            Some(&holding(DEFINITION, 100)),
            &Message::Burn { amount: 30 },
        );
        assert_eq!(
            response,
            write(&holding(DEFINITION, 70))
                .send_sync(token(DEFINITION), &Internal::DecreaseSupply { amount: 30 })
        );
    }

    #[test]
    #[should_panic(expected = "holding must authorize the burn")]
    fn refuses_an_unauthorized_holding() {
        let _ = run(
            HOLDING,
            None,
            false,
            Some(&holding(DEFINITION, 100)),
            &Message::Burn { amount: 30 },
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
            &Message::Burn { amount: 101 },
        );
    }

    #[test]
    fn the_definition_reduces_its_supply() {
        let response = run(
            DEFINITION,
            Some(token(HOLDING)),
            false,
            Some(&definition(1_000, None)),
            &Internal::DecreaseSupply { amount: 30 },
        );
        let event = Burned {
            definition: DEFINITION,
            holding: HOLDING,
            amount: 30,
        };
        assert_eq!(
            response,
            write(&definition(970, None)).event(event.to_program_event())
        );
    }

    #[test]
    #[should_panic(expected = "burn exceeds total supply")]
    fn the_definition_refuses_an_underflow() {
        let _ = run(
            DEFINITION,
            Some(token(HOLDING)),
            false,
            Some(&definition(10, None)),
            &Internal::DecreaseSupply { amount: 11 },
        );
    }

    #[test]
    #[should_panic(expected = "receiver is not a token definition")]
    fn decrease_supply_fails_at_a_holding() {
        let _ = run(
            RECIPIENT,
            Some(token(HOLDING)),
            false,
            Some(&holding(DEFINITION, 10)),
            &Internal::DecreaseSupply { amount: 1 },
        );
    }
}
