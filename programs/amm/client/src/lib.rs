//! Stateless AMM quoting and transaction planning for host consumers.

pub mod error;
mod ffi;
pub mod plan;
pub mod quote;
pub mod slippage;
pub mod wire;

pub use error::ClientError;
pub use ffi::{amm_client_free, amm_client_plan, amm_client_quote};
pub use plan::{
    encode_instruction, plan_add_liquidity, plan_create_oracle_price_account, plan_create_pool,
    plan_create_price_observations, plan_initialize, plan_remove_liquidity, plan_swap_exact_input,
    plan_swap_exact_output, plan_sync_reserves, plan_update_config, AccountRole,
    AddLiquidityPlanInput, AmmContext, CreateOraclePriceAccountPlanInput, CreatePoolPlanInput,
    CreatePriceObservationsPlanInput, InitializePlanInput, PlannedAccount, PoolContext,
    RemoveLiquidityPlanInput, SwapExactInputPlanInput, SwapExactOutputPlanInput,
    SyncReservesPlanInput, TransactionPlan, UpdateConfigPlanInput,
};
pub use slippage::{
    maximum_guard_amount, minimum_guard_amount, prepare_add_liquidity, prepare_create_pool,
    prepare_remove_liquidity, prepare_swap_exact_input, prepare_swap_exact_output,
    PreparedAddLiquidity, PreparedCreatePool, PreparedRemoveLiquidity, PreparedSwapExactInput,
    PreparedSwapExactOutput, SlippageTolerance, SLIPPAGE_BPS_DENOMINATOR,
};
