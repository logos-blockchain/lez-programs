use amm_core::{
    compute_protocol_fee_pda, compute_protocol_fee_pda_seed, compute_vault_pda_seed, event::Event,
    protocol_fee_amount, swap_exact_in_amounts, swap_exact_out_amounts, Message, Pending,
};
use lee_core::program::{ReceiveInput, Response};
use token_core::Message as TokenMessage;

use crate::{
    reads,
    settlement::{self, token_call},
};

pub(crate) fn execute(input: &ReceiveInput, pending: &Pending) -> Response {
    let mut pool = reads::pool(pending);
    let [source, target, _, _] = pending.holdings.as_slice() else {
        unreachable!("four holdings read")
    };
    let source = source.as_ref().expect("input holding must exist");
    let a_to_b = if source.definition_id == pool.definition_token_a_id {
        true
    } else {
        assert_eq!(
            source.definition_id, pool.definition_token_b_id,
            "input token is not part of the pool"
        );
        false
    };
    let (reserve_in, reserve_out, vault_in, vault_out, token_out) = if a_to_b {
        (
            pool.reserve_a,
            pool.reserve_b,
            pool.vault_a_id,
            pool.vault_b_id,
            pool.definition_token_b_id,
        )
    } else {
        (
            pool.reserve_b,
            pool.reserve_a,
            pool.vault_b_id,
            pool.vault_a_id,
            pool.definition_token_a_id,
        )
    };
    reads::holding(target, token_out);
    assert!(
        reserve_in != 0 && reserve_out != 0,
        "reserves must be nonzero"
    );
    let config = pending.config.as_ref().expect("config was read");
    let (from, to, amount_in, amount_out, effective) = match pending.request {
        Message::SwapExactInput {
            user_input_holding,
            user_output_holding,
            swap_amount_in,
            min_amount_out,
            ..
        } => {
            let (effective, out) =
                swap_exact_in_amounts(swap_amount_in, reserve_in, reserve_out, config.swap_fee_bps);
            assert!(out >= min_amount_out, "output below minimum");
            (
                user_input_holding,
                user_output_holding,
                swap_amount_in,
                out,
                effective,
            )
        }
        Message::SwapExactOutput {
            user_input_holding,
            user_output_holding,
            exact_amount_out,
            max_amount_in,
            ..
        } => {
            let (effective, amount_in) = swap_exact_out_amounts(
                exact_amount_out,
                reserve_in,
                reserve_out,
                config.swap_fee_bps,
            )
            .expect("unfulfillable exact output");
            assert!(amount_in <= max_amount_in, "input exceeds maximum");
            (
                user_input_holding,
                user_output_holding,
                amount_in,
                exact_amount_out,
                effective,
            )
        }
        Message::Initialize { .. }
        | Message::UpdateConfig { .. }
        | Message::NewDefinition { .. }
        | Message::AddLiquidity { .. }
        | Message::RemoveLiquidity { .. }
        | Message::SyncReserves
        | Message::CreatePriceObservations { .. }
        | Message::CreateOraclePriceAccount { .. }
        | Message::WithdrawProtocolFees { .. }
        | Message::ReadState => unreachable!("swap continuation"),
    };
    assert!(
        effective != 0 && amount_out != 0,
        "swap amounts must be nonzero"
    );
    assert!(source.balance >= amount_in, "insufficient input balance");
    let swap_fee = amount_in
        .checked_sub(effective)
        .expect("effective input is at most gross input");
    let protocol_fee = protocol_fee_amount(swap_fee, config.protocol_fee_bps);
    let net = amount_in
        .checked_sub(protocol_fee)
        .expect("protocol fee is part of input");
    let new_in = reserve_in.checked_add(net).expect("input reserve overflow");
    let new_out = reserve_out
        .checked_sub(amount_out)
        .expect("output reserve underflow");
    (pool.reserve_a, pool.reserve_b) = if a_to_b {
        (new_in, new_out)
    } else {
        (new_out, new_in)
    };
    let mut response = Response::keep_state()
        .send(token_call(
            config,
            from,
            &TokenMessage::Transfer {
                to: vault_in,
                amount: amount_in,
            },
            None,
        ))
        .send(token_call(
            config,
            vault_out,
            &TokenMessage::Transfer {
                to,
                amount: amount_out,
            },
            Some(compute_vault_pda_seed(input.receiver.account_id, token_out)),
        ));
    if protocol_fee != 0 {
        let fee_holding = compute_protocol_fee_pda(
            input.receiver.program_account_id,
            pending.config_id,
            source.definition_id,
        );
        response = response
            .send(token_call(
                config,
                fee_holding,
                &TokenMessage::InitializeHolding {
                    definition: source.definition_id,
                },
                Some(compute_protocol_fee_pda_seed(
                    pending.config_id,
                    source.definition_id,
                )),
            ))
            .send(token_call(
                config,
                vault_in,
                &TokenMessage::Transfer {
                    to: fee_holding,
                    amount: protocol_fee,
                },
                Some(compute_vault_pda_seed(
                    input.receiver.account_id,
                    source.definition_id,
                )),
            ));
    }
    response = response.event(
        Event::Swapped {
            token_in: source.definition_id,
            amount_in,
            amount_out,
            protocol_fee,
        }
        .to_program_event(),
    );
    settlement::pool(input, config, pool, response)
}
