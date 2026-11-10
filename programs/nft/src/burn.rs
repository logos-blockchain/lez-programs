//! `Burn`: the holding destroys its value and tells its definition to lower the supply.

use lee_core::{
    account::Actor,
    program::{ReceiveInput, Response},
};
use nft_core::{
    event::{Burned, NftEvent as _},
    Internal, NftCopy, NftMaster, NftState,
};

use crate::{definition_state, nft_actor};

/// At the holding: up to the print balance of a master (down to 0, as in v0.2.5), exactly 1
/// of an owned copy.
pub(crate) fn burn(input: &ReceiveInput, amount: u128) -> Response {
    assert!(input.is_authorized, "holding must authorize the burn");
    let (definition, burned) = match NftState::decode(&input.pre_state) {
        Some(NftState::Master(master)) => {
            let print_balance = master
                .print_balance
                .checked_sub(amount)
                .expect("insufficient print balance");
            let burned = NftState::Master(NftMaster {
                print_balance,
                ..master
            });
            (master.definition_id, burned)
        }
        Some(NftState::Copy(copy)) => {
            assert!(copy.owned, "holding holds no NFT");
            assert!(amount == 1, "a copy burns exactly 1");
            let burned = NftState::Copy(NftCopy {
                owned: false,
                ..copy
            });
            (copy.definition_id, burned)
        }
        Some(NftState::Definition(_)) | None => panic!("receiver is not an NFT holding"),
    };
    Response::set_state(burned.encode()).send_sync(
        nft_actor(input, definition),
        &Internal::DecreaseSupply { amount },
    )
}

/// At the holding's definition.
pub(crate) fn decrease_supply(input: &ReceiveInput, from: Actor, amount: u128) -> Response {
    let mut definition = definition_state(input);
    definition.printable_supply = definition
        .printable_supply
        .checked_sub(amount)
        .expect("burn exceeds printable supply");
    let event = Burned {
        definition: input.receiver.account_id,
        holding: from.account_id,
        amount,
    };
    Response::set_state(NftState::Definition(definition).encode()).event(event.to_program_event())
}

#[cfg(test)]
mod tests {
    mod burn {
        use crate::test_utils::*;

        fn message(amount: u128) -> Message {
            Message::Burn { amount }
        }

        /// Supply invariant: `DecreaseSupply` follows the matching decrement.
        #[test]
        fn a_master_burns_part_of_its_balance() {
            let response = run(
                MASTER,
                None,
                true,
                Some(&master(DEFINITION, 10)),
                &message(3),
            );
            assert_eq!(
                response,
                write(&master(DEFINITION, 7))
                    .send_sync(nft(DEFINITION), &Internal::DecreaseSupply { amount: 3 })
            );
        }

        /// v0.2.5 behaviour: a master may burn its own unit too.
        #[test]
        fn a_master_burns_its_whole_balance() {
            let response = run(
                MASTER,
                None,
                true,
                Some(&master(DEFINITION, 10)),
                &message(10),
            );
            assert_eq!(
                response,
                write(&master(DEFINITION, 0))
                    .send_sync(nft(DEFINITION), &Internal::DecreaseSupply { amount: 10 })
            );
        }

        #[test]
        fn an_owned_copy_burns_itself() {
            let response = run(
                HOLDING,
                None,
                true,
                Some(&copy(DEFINITION, true)),
                &message(1),
            );
            assert_eq!(
                response,
                write(&copy(DEFINITION, false))
                    .send_sync(nft(DEFINITION), &Internal::DecreaseSupply { amount: 1 })
            );
        }

        #[test]
        #[should_panic(expected = "holding must authorize the burn")]
        fn refuses_an_unauthorized_holding() {
            let _ = run(
                HOLDING,
                None,
                false,
                Some(&copy(DEFINITION, true)),
                &message(1),
            );
        }

        #[test]
        #[should_panic(expected = "insufficient print balance")]
        fn a_master_refuses_more_than_its_balance() {
            let _ = run(
                MASTER,
                None,
                true,
                Some(&master(DEFINITION, 10)),
                &message(11),
            );
        }

        #[test]
        #[should_panic(expected = "a copy burns exactly 1")]
        fn a_copy_refuses_another_amount() {
            let _ = run(
                HOLDING,
                None,
                true,
                Some(&copy(DEFINITION, true)),
                &message(2),
            );
        }

        #[test]
        #[should_panic(expected = "holding holds no NFT")]
        fn a_copy_it_does_not_own_refuses() {
            let _ = run(
                HOLDING,
                None,
                true,
                Some(&copy(DEFINITION, false)),
                &message(1),
            );
        }

        #[test]
        #[should_panic(expected = "receiver is not an NFT holding")]
        fn fails_at_a_definition() {
            let _ = run(DEFINITION, None, true, Some(&definition(10)), &message(1));
        }
    }

    mod decrease_supply {
        use crate::test_utils::*;

        #[test]
        fn the_definition_lowers_its_supply() {
            let response = run(
                DEFINITION,
                Some(nft(HOLDING)),
                false,
                Some(&definition(100)),
                &Internal::DecreaseSupply { amount: 1 },
            );
            let event = Burned {
                definition: DEFINITION,
                holding: HOLDING,
                amount: 1,
            };
            assert_eq!(
                response,
                write(&definition(99)).event(event.to_program_event())
            );
        }

        #[test]
        #[should_panic(expected = "burn exceeds printable supply")]
        fn refuses_an_underflow() {
            let _ = run(
                DEFINITION,
                Some(nft(HOLDING)),
                false,
                Some(&definition(1)),
                &Internal::DecreaseSupply { amount: 2 },
            );
        }

        #[test]
        #[should_panic(expected = "receiver is not an NFT definition")]
        fn fails_at_a_holding() {
            let _ = run(
                RECIPIENT,
                Some(nft(HOLDING)),
                false,
                Some(&copy(DEFINITION, false)),
                &Internal::DecreaseSupply { amount: 1 },
            );
        }
    }
}
