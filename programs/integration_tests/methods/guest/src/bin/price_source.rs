//! A stand-in price source, as the AMM is for the TWAP oracle. Each actor forwards what a root
//! message asks, granting the given seeds, and keeps the last message another actor sent it, so
//! a test can read a reply. It checks nothing: it exists only for the integration tests.

use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::{
    account::Actor,
    program::{PdaSeed, ReceiveInput, Response, SyncMessage},
};

/// The tests encode the same enum (`tests/twap_oracle.rs`).
#[derive(BorshSerialize, BorshDeserialize)]
enum SourceMessage {
    /// Send `message` (already encoded) to `to`, granting `seeds`.
    Forward {
        to: Actor,
        message: Vec<u8>,
        seeds: Vec<PdaSeed>,
    },
}

lee_core::actor_entrypoint!(raw handle_message);

fn handle_message(input: &ReceiveInput) -> Response {
    if input.from.is_some() {
        return Response::set_state(input.message.clone());
    }
    let SourceMessage::Forward { to, message, seeds } =
        borsh::from_slice(&input.message).expect("price source message must decode");
    Response::keep_state().send(SyncMessage {
        to,
        message,
        pda_seeds: seeds.into_iter().collect(),
    })
}
