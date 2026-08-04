#![deny(unsafe_op_in_unsafe_fn)]

mod account;
mod ffi;

pub mod api;

pub use api::{
    config_id, context, pair_ids, plan, pool_id, program_id, quote, resolve_pool, swap_pair,
    swap_plan, token_ids, AccountRead, AmmApiError, AmmResponse, AmmResult, ConfigIdRequest,
    ContextRequest, PairIdsRequest, PairSnapshot, PlanRequest, PoolIdRequest, PositionRequest,
    ProgramIdRequest, QuoteRequest, ResolvePoolRequest, SwapPairRequest, SwapPlanRequest,
    TokenIdsRequest, WalletAccount,
};
