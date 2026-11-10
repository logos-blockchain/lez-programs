//! `ReadState`: other programs can't read NFT state, so they ask (`token.md` §3.9).

use lee_core::program::{ReceiveInput, Response, StateReply};

/// At any NFT actor: reply with its state to the sender.
pub(crate) fn read_state(input: &ReceiveInput) -> Response {
    let reader = input.from.expect("ReadState needs a sender to reply to");
    Response::keep_state().send_sync(reader, &StateReply::from(input))
}

#[cfg(test)]
mod tests {
    use crate::test_utils::*;

    #[test]
    fn replies_with_the_state_to_the_sender() {
        let state = definition(100);
        let response = run(
            DEFINITION,
            Some(outsider(HOLDING)),
            false,
            Some(&state),
            &Message::ReadState,
        );
        assert_eq!(
            response,
            Response::keep_state().send_sync(
                outsider(HOLDING),
                &StateReply {
                    state: ActorState::from(state.encode()),
                }
            )
        );
    }

    #[test]
    #[should_panic(expected = "ReadState needs a sender to reply to")]
    fn needs_a_sender() {
        let _ = run(HOLDING, None, false, None, &Message::ReadState);
    }
}
