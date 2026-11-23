//! Fixtures for the handler tests: one hand-built `ReceiveInput` per message, compared against
//! the expected `Response`. Conversations between actors are covered by the integration tests.

pub(crate) use borsh::BorshSerialize;
pub(crate) use clock_core::Reply;
pub(crate) use lee_core::{
    account::{AccountId, Actor, ActorState},
    program::{ReceiveInput, Response, SyncMessage},
};
pub(crate) use stablecoin_core::{
    compute_engine_pda, compute_position_vault_pda, compute_stablecoin_definition_pda,
    compute_surplus_holding_pda, Engine, Message, Pending, Position, ProtocolParameters,
    RedemptionPriceState, StabilityFeeAccumulator, StablecoinState, FIXED_POINT_ONE,
};
pub(crate) use twap_oracle_core::{OraclePrice, PriceReply};

use crate::handle_message;

pub(crate) const STABLECOIN: AccountId = AccountId::new([0xAA; 32]);
pub(crate) const TOKEN: AccountId = AccountId::new([0xBB; 32]);
pub(crate) const ORACLE_PROGRAM: AccountId = AccountId::new([0xCC; 32]);
pub(crate) const OTHER_PROGRAM: AccountId = AccountId::new([0xDD; 32]);

pub(crate) const OWNER: AccountId = AccountId::new([1; 32]);
pub(crate) const ADMIN: AccountId = AccountId::new([2; 32]);
pub(crate) const FREEZE_AUTHORITY: AccountId = AccountId::new([3; 32]);
pub(crate) const COLLATERAL_DEFINITION: AccountId = AccountId::new([4; 32]);
pub(crate) const ORACLE_PRICE: AccountId = AccountId::new([5; 32]);
pub(crate) const HOLDING: AccountId = AccountId::new([6; 32]);
pub(crate) const RECIPIENT: AccountId = AccountId::new([7; 32]);

pub(crate) fn engine_id() -> AccountId {
    compute_engine_pda(STABLECOIN)
}

pub(crate) fn definition_id() -> AccountId {
    compute_stablecoin_definition_pda(STABLECOIN)
}

pub(crate) fn surplus_id() -> AccountId {
    compute_surplus_holding_pda(STABLECOIN)
}

pub(crate) fn vault_id(owner: AccountId) -> AccountId {
    compute_position_vault_pda(STABLECOIN, owner)
}

/// A stablecoin actor on `account_id`.
pub(crate) fn stablecoin(account_id: AccountId) -> Actor {
    Actor::new(account_id, STABLECOIN)
}

/// A token-program actor on `account_id`.
pub(crate) fn token(account_id: AccountId) -> Actor {
    Actor::new(account_id, TOKEN)
}

/// The engine's actor.
pub(crate) fn engine() -> Actor {
    stablecoin(engine_id())
}

pub(crate) fn clock() -> Actor {
    clock_core::clock_actor()
}

/// The market price oracle's actor, as `parameters()` configures it.
pub(crate) fn oracle() -> Actor {
    Actor::new(ORACLE_PRICE, ORACLE_PROGRAM)
}

/// An actor of an unrelated program.
pub(crate) fn outsider(account_id: AccountId) -> Actor {
    Actor::new(account_id, OTHER_PROGRAM)
}

pub(crate) fn run<M: BorshSerialize>(
    receiver: Actor,
    from: Option<Actor>,
    is_authorized: bool,
    pre: Option<&StablecoinState>,
    message: &M,
) -> Response {
    let input = ReceiveInput {
        receiver,
        from,
        is_authorized,
        pre_state: ActorState::from(pre.map(StablecoinState::encode).unwrap_or_default()),
        message: borsh::to_vec(message).expect("test message encodes"),
    };
    handle_message(&input)
}

/// Valid parameters: a minimum collateralization ratio of 1.5x, a minimum interval of 1 s and a
/// maximum oracle age of 10 s.
pub(crate) fn parameters() -> ProtocolParameters {
    ProtocolParameters {
        admin_account_id: ADMIN,
        freeze_authority_account_id: FREEZE_AUTHORITY,
        token_program_id: TOKEN,
        stablecoin_definition_id: definition_id(),
        collateral_definition_id: COLLATERAL_DEFINITION,
        market_price_oracle_id: ORACLE_PRICE,
        market_price_oracle_program_id: ORACLE_PROGRAM,
        stability_fee_per_millisecond: FIXED_POINT_ONE,
        controller_proportional_gain: 0,
        controller_integral_gain: 0,
        minimum_collateralization_ratio: FIXED_POINT_ONE * 3 / 2,
        minimum_milliseconds_between_rate_updates: 1_000,
        maximum_oracle_price_age_milliseconds: 10_000,
        is_frozen: false,
        minimum_debt_amount: 10,
        debt_ceiling: 1_000_000,
    }
}

/// A valid engine with nothing pending, last accrued and last updated at `now`.
pub(crate) fn engine_at(now: u64) -> Engine {
    Engine {
        parameters: parameters(),
        fee: StabilityFeeAccumulator {
            accumulated_rate_at_last_accrual: FIXED_POINT_ONE,
            last_accrued_at: now,
            total_normalized_debt: 0,
        },
        redemption: RedemptionPriceState {
            redemption_price_at_last_update: FIXED_POINT_ONE,
            redemption_rate_per_millisecond: FIXED_POINT_ONE,
            controller_integral_term: 0,
            last_updated_at: now,
        },
        pending: None,
    }
}

pub(crate) fn clock_reply(now: u64) -> Reply {
    Reply::Timestamp(now)
}

/// The oracle's reply to `ReadPrice`: the stablecoin priced in collateral.
pub(crate) fn price_reply(price: u128, timestamp: u64) -> PriceReply {
    PriceReply {
        price: OraclePrice {
            base_asset: definition_id(),
            quote_asset: COLLATERAL_DEFINITION,
            price,
            timestamp,
            source_id: ORACLE_PRICE,
            confidence_interval: 0,
        },
    }
}

/// Decodes the engine the response saves.
pub(crate) fn saved_engine(response: &Response) -> Engine {
    let state = response.post_state.clone().expect("state is set");
    match StablecoinState::decode(&state).expect("state decodes") {
        StablecoinState::Engine(engine) => engine,
        StablecoinState::Position(_) => panic!("the engine stays an engine"),
    }
}
