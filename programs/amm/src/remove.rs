use amm_core::{
    compute_vault_pda_seed, event::Event, mul_div_floor, Message, Pending, MINIMUM_LIQUIDITY,
};
use lee_core::program::{ReceiveInput, Response};
use token_core::Message as TokenMessage;

use crate::{
    reads,
    settlement::{self, token_call},
};

pub(crate) fn execute(input: &ReceiveInput, pending: &Pending) -> Response {
    let Message::RemoveLiquidity {
        user_holding_a,
        user_holding_b,
        user_holding_lp,
        remove_liquidity_amount: liquidity,
        min_amount_to_remove_token_a: min_a,
        min_amount_to_remove_token_b: min_b,
        ..
    } = pending.request
    else {
        unreachable!("remove continuation")
    };
    let mut pool = reads::pool(pending);
    let [a, b, lp, _, _] = pending.holdings.as_slice() else {
        unreachable!("five holdings read")
    };
    reads::holding(a, pool.definition_token_a_id);
    reads::holding(b, pool.definition_token_b_id);
    let lp = reads::holding(lp, pool.liquidity_pool_id);
    assert!(
        liquidity != 0 && min_a != 0 && min_b != 0,
        "withdrawal amounts must be nonzero"
    );
    assert!(liquidity <= lp.balance, "insufficient LP balance");
    let unlocked = pool
        .liquidity_pool_supply
        .checked_sub(MINIMUM_LIQUIDITY)
        .expect("minimum liquidity is locked");
    assert!(
        liquidity <= unlocked,
        "cannot remove locked minimum liquidity"
    );
    let amount_a = mul_div_floor(pool.reserve_a, liquidity, pool.liquidity_pool_supply);
    let amount_b = mul_div_floor(pool.reserve_b, liquidity, pool.liquidity_pool_supply);
    assert!(
        amount_a >= min_a && amount_b >= min_b,
        "withdrawal below minimum"
    );
    pool.liquidity_pool_supply = pool
        .liquidity_pool_supply
        .checked_sub(liquidity)
        .expect("LP supply underflow");
    pool.reserve_a = pool
        .reserve_a
        .checked_sub(amount_a)
        .expect("reserve A underflow");
    pool.reserve_b = pool
        .reserve_b
        .checked_sub(amount_b)
        .expect("reserve B underflow");
    let config = pending.config.as_ref().expect("config was read");
    // Burn synchronously reports the supply decrease to the LP definition.
    let response = Response::keep_state()
        .send(token_call(
            config,
            user_holding_lp,
            &TokenMessage::Burn { amount: liquidity },
            None,
        ))
        .send(token_call(
            config,
            pool.vault_a_id,
            &TokenMessage::Transfer {
                to: user_holding_a,
                amount: amount_a,
            },
            Some(compute_vault_pda_seed(
                input.receiver.account_id,
                pool.definition_token_a_id,
            )),
        ))
        .send(token_call(
            config,
            pool.vault_b_id,
            &TokenMessage::Transfer {
                to: user_holding_b,
                amount: amount_b,
            },
            Some(compute_vault_pda_seed(
                input.receiver.account_id,
                pool.definition_token_b_id,
            )),
        ))
        .event(
            Event::LiquidityRemoved {
                amount_a,
                amount_b,
                liquidity,
            }
            .to_program_event(),
        );
    settlement::pool(input, config, pool, response)
}
