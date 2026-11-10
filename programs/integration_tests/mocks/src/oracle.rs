//! Records AMM oracle messages. No clock, observations, tick math or TWAP calculation:
//! this deliberately models only the caller contract, not the unported oracle.

use amm_core::oracle::Message;
use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::{
    account::Actor,
    program::{ReceiveInput, Response},
};

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct OracleState {
    pub source: Actor,
    pub price: u128,
    pub observations: Vec<u64>,
    pub prices: Vec<Message>,
    /// Fault injection for integration tests that verify whole-transaction rollback.
    pub reject_updates: bool,
}

pub fn handle_message(input: &ReceiveInput) -> Response {
    let from = input.from.expect("oracle needs a source actor");
    assert_eq!(
        from.account_id, input.receiver.account_id,
        "oracle source must be its pool"
    );
    assert!(input.is_authorized, "oracle source must authorize");
    let message: Message = borsh::from_slice(&input.message).expect("oracle message decodes");
    let state = if let Message::CreateCurrentTick { price } = message {
        assert!(input.pre_state.is_empty(), "oracle already initialized");
        OracleState {
            source: from,
            price,
            observations: vec![],
            prices: vec![],
            reject_updates: false,
        }
    } else {
        let mut state: OracleState =
            borsh::from_slice(&input.pre_state).expect("oracle must be initialized");
        assert_eq!(state.source, from, "oracle source program mismatch");
        match message {
            Message::UpdateCurrentTick { price } => {
                assert!(!state.reject_updates, "mock oracle refused update");
                state.price = price;
            }
            Message::CreatePriceObservations { window_duration } => {
                assert!(
                    !state.observations.contains(&window_duration),
                    "observations already exist"
                );
                state.observations.push(window_duration);
            }
            Message::CreateOraclePriceAccount {
                window_duration, ..
            } => {
                assert!(!state.prices.iter().any(|m| matches!(m, Message::CreateOraclePriceAccount { window_duration: old, .. } if *old == window_duration)), "oracle price already exists");
                state.prices.push(message);
            }
            Message::CreateCurrentTick { .. } => unreachable!("creation handled above"),
        }
        state
    };
    Response::set_state(borsh::to_vec(&state).expect("mock state encodes"))
}
