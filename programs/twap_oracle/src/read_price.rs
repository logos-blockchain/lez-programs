//! `ReadPrice`: other programs can't read the price actor, so they ask.

use lee_core::program::{ReceiveInput, Response};
use twap_oracle_core::PriceReply;

use crate::price_state;

/// At a price actor: reply with the price to the sender.
pub(crate) fn read_price(input: &ReceiveInput) -> Response {
    let reader = input.from.expect("ReadPrice needs a sender to reply to");
    let price = price_state(input);
    Response::keep_state().send_sync(reader, &PriceReply { price })
}

#[cfg(test)]
mod tests {
    use crate::test_utils::*;

    #[test]
    fn replies_with_the_price_to_the_sender() {
        let response = run(
            price_id(),
            Some(outsider(READER)),
            false,
            Some(&price(ONE, 3_000)),
            &ReadPrice,
        );
        assert_eq!(
            response,
            Response::keep_state().send_sync(
                outsider(READER),
                &PriceReply {
                    price: oracle_price(ONE, 3_000)
                }
            )
        );
    }

    #[test]
    #[should_panic(expected = "ReadPrice needs a sender to reply to")]
    fn a_root_read_panics() {
        let _ = run(
            price_id(),
            None,
            false,
            Some(&price(ONE, 3_000)),
            &ReadPrice,
        );
    }

    #[test]
    #[should_panic(expected = "receiver is not a price actor")]
    fn a_read_at_the_current_tick_actor_panics() {
        let pre = current_tick(5, 0, None);
        let _ = run(
            current_tick_id(),
            Some(outsider(READER)),
            false,
            Some(&pre),
            &ReadPrice,
        );
    }

    #[test]
    #[should_panic(expected = "receiver is not a price actor")]
    fn a_read_at_the_observations_actor_panics() {
        let pre = observations_at(&[(1_000, 0)], 0);
        let _ = run(
            observations_id(),
            Some(outsider(READER)),
            false,
            Some(&pre),
            &ReadPrice,
        );
    }
}
