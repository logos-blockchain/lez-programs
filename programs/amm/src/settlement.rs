use amm_core::{compute_pool_pda_seed, oracle, AmmConfig, AmmState, Internal, PoolDefinition};
use lee_core::{
    account::{AccountId, Actor},
    program::{PdaSeed, ReceiveInput, Response, SyncMessage},
};

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
    pool: &PoolDefinition,
    message: &oracle::Message,
) -> SyncMessage {
    SyncMessage::new(
        Actor::new(input.receiver.account_id, config.twap_oracle_program_id),
        message,
    )
    .with_pda_seeds([compute_pool_pda_seed(
        pool.config_id,
        pool.definition_token_a_id,
        pool.definition_token_b_id,
    )])
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
        &pool,
        &oracle::Message::UpdateCurrentTick { price },
    ));
    finish(input, AmmState::Pool(pool), response)
}
