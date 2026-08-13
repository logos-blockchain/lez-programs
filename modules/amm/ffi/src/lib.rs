#![deny(unsafe_op_in_unsafe_fn)]

mod account;
mod ffi;

pub mod api;

pub use api::{
    config_account, config_id, create_pool_plan, create_pool_quote, fee_tiers, pair_ids, pool_id,
    program_id, resolve_pool, resolve_tokens, swap_exact_in_plan, swap_exact_in_quote,
    swap_exact_out_plan, swap_exact_out_quote, swap_pair, AccountRead, AmmApiError, AmmResponse,
    AmmResult, ConfigAccountRequest, ConfigIdRequest, CreatePoolPlanRequest,
    CreatePoolQuoteRequest, FeeTiersRequest, PairIdsRequest, PoolIdRequest, ProgramIdRequest,
    ResolvePoolRequest, ResolveTokensRequest, SwapExactInPlanRequest, SwapExactInQuoteRequest,
    SwapExactOutPlanRequest, SwapExactOutQuoteRequest, SwapPairRequest, WalletAccount,
};
