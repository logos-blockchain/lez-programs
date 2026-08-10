#![deny(unsafe_op_in_unsafe_fn)]

mod account;
mod ffi;

pub mod api;

pub use api::{
    config_id, context, create_pool_plan, liquidity_quote, pair_ids, pool_id, program_id,
    resolve_pool, swap_exact_in_plan, swap_exact_in_quote, swap_exact_out_plan,
    swap_exact_out_quote, swap_pair, token_ids, AccountRead, AmmApiError, AmmResponse, AmmResult,
    ConfigIdRequest, ContextRequest, CreatePoolPlanRequest, LiquidityQuoteRequest, PairIdsRequest,
    PoolIdRequest, ProgramIdRequest, ResolvePoolRequest, SwapExactInPlanRequest,
    SwapExactInQuoteRequest, SwapExactOutPlanRequest, SwapExactOutQuoteRequest, SwapPairRequest,
    TokenIdsRequest, WalletAccount,
};
