use amm_core::{AmmConfig, AmmState, Internal, PoolDefinition};
use lee_core::{
    account::{AccountId, Actor},
    program::{PdaSeed, ReceiveInput, Response, SyncMessage},
};
use twap_oracle_core::{compute_current_tick_account_pda_seed, Message as OracleMessage};

pub(crate) fn token_call(
    config: &AmmConfig,
    account: AccountId,
    message: &token_core::Message,
    seed: Option<PdaSeed>,
) -> SyncMessage {
    SyncMessage::new(Actor::new(account, config.token_program_id), message).with_pda_seeds(seed)
}

pub(crate) fn oracle_call(
    input: &ReceiveInput,
    config: &AmmConfig,
    message: &OracleMessage,
) -> SyncMessage {
    let seed = compute_current_tick_account_pda_seed(input.receiver.account_id);
    let tick = AccountId::for_public_pda(&input.receiver.program_account_id, &seed);
    // Admit the tick PDA at creation. Subsequent calls authenticate the sending pool actor.
    let admission =
        matches!(message, OracleMessage::CreateCurrentTickAccount { .. }).then_some(seed);
    SyncMessage::new(Actor::new(tick, config.twap_oracle_program_id), message)
        .with_pda_seeds(admission)
}

pub(crate) fn finish(input: &ReceiveInput, state: AmmState, mut response: Response) -> Response {
    response.post_state = Some(AmmState::Settling(Box::new(state)).encode().into());
    response.send_sync(input.receiver, &Internal::Complete)
}

pub(crate) fn pool(
    input: &ReceiveInput,
    config: &AmmConfig,
    pool: PoolDefinition,
    response: Response,
) -> Response {
    let price = amm_core::spot_price_q64_64(pool.reserve_a, pool.reserve_b);
    let response = response.send(oracle_call(
        input,
        config,
        &OracleMessage::UpdateCurrentTick { price },
    ));
    finish(input, AmmState::Pool(pool), response)
}
