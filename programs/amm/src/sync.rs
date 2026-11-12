use amm_core::{event::Event, oracle, spot_price_q64_64, AmmState, Message, Pending};
use lee_core::program::{ReceiveInput, Response};

use crate::{reads, settlement};

pub(crate) fn execute(input: &ReceiveInput, pending: &Pending) -> Response {
    let mut pool = reads::pool(pending);
    let config = pending.config.as_ref().expect("config was read");
    let message = match pending.request {
        Message::SyncReserves => {
            let [a, b] = pending.holdings.as_slice() else {
                unreachable!("two vaults read")
            };
            pool.reserve_a = reads::holding(a, pool.definition_token_a_id).balance;
            pool.reserve_b = reads::holding(b, pool.definition_token_b_id).balance;
            let event = Event::ReservesSynced {
                reserve_a: pool.reserve_a,
                reserve_b: pool.reserve_b,
            };
            return settlement::pool(
                input,
                config,
                pool,
                Response::keep_state().event(event.to_program_event()),
            );
        }
        Message::CreatePriceObservations { window_duration } => {
            assert!(
                window_duration >= oracle::MIN_WINDOW_DURATION,
                "oracle window is too short"
            );
            oracle::Message::CreatePriceObservations { window_duration }
        }
        Message::CreateOraclePriceAccount { window_duration } => {
            assert!(
                window_duration >= oracle::MIN_WINDOW_DURATION,
                "oracle window is too short"
            );
            let initial_price = spot_price_q64_64(pool.reserve_a, pool.reserve_b);
            assert!(initial_price != 0, "oracle price must be nonzero");
            oracle::Message::CreateOraclePriceAccount {
                base_asset: pool.definition_token_a_id,
                quote_asset: pool.definition_token_b_id,
                initial_price,
                window_duration,
            }
        }
        Message::Initialize { .. }
        | Message::UpdateConfig { .. }
        | Message::NewDefinition { .. }
        | Message::AddLiquidity { .. }
        | Message::RemoveLiquidity { .. }
        | Message::SwapExactInput { .. }
        | Message::SwapExactOutput { .. }
        | Message::WithdrawProtocolFees { .. }
        | Message::ReadState => unreachable!("sync or oracle continuation"),
    };
    let response =
        Response::keep_state().send(settlement::oracle_call(input, config, &pool, &message));
    settlement::finish(input, AmmState::Pool(pool), response)
}
