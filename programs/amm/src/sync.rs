use amm_core::{event::Event, AmmState, Message, Pending};
use lee_core::program::{ReceiveInput, Response};
use twap_oracle_core::Message as OracleMessage;

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
        Message::CreatePriceObservations { window_duration }
        | Message::CreateOraclePriceAccount { window_duration } => {
            assert!(
                window_duration >= u64::from(twap_oracle_core::OBSERVATIONS_CAPACITY),
                "oracle window is too short"
            );
            OracleMessage::CreatePriceAccounts {
                base_asset: pool.definition_token_a_id,
                quote_asset: pool.definition_token_b_id,
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
    let response = Response::keep_state().send(settlement::oracle_call(input, config, &message));
    settlement::finish(input, AmmState::Pool(pool), response)
}
