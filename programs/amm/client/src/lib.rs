//! Stateless AMM quoting and transaction planning for host consumers.

pub mod discovery;
pub mod error;
mod ffi;
pub mod intent;
pub mod plan;
pub mod quote;
pub mod slippage;
pub mod transaction;
pub mod wire;

pub use discovery::{
    canonical_pair, derive_config_id, derive_pair_read_manifest, inspect_config, inspect_pair,
    ActivePairInspection, CanonicalPair, MissingPairInspection, MissingVaultState, PairInspection,
    PairReadManifest, PairReadSnapshots, TokenReadManifest, ValidatedClockSnapshot,
};
pub use error::ClientError;
pub use ffi::{amm_client_free, amm_client_plan, amm_client_quote};
pub use intent::{
    caller_amounts_to_stored, human_price_ratio_to_q64_64, paired_amount_from_token_a,
    paired_amount_from_token_b, pool_spot_change_bps, prepare_caller_opening_pair,
    prepare_minimum_opening_pair, prepare_opening_from_token_a, prepare_opening_from_token_b,
    stored_amounts_to_caller, validate_explicit_opening_pair, IntentError, OpeningLiquidityIntent,
    PreparedCallerOpeningPair, PreparedOpeningPair, MAX_HUMAN_PRICE_FRACTIONAL_DIGITS,
    MAX_TOKEN_DECIMALS, Q64_64_ONE,
};
pub use plan::{
    encode_instruction, plan_add_liquidity, plan_create_oracle_price_account, plan_create_pool,
    plan_create_price_observations, plan_initialize, plan_remove_liquidity, plan_swap_exact_input,
    plan_swap_exact_output, plan_sync_reserves, plan_update_config, AccountRole,
    AddLiquidityPlanInput, AmmContext, CreateOraclePriceAccountPlanInput, CreatePoolPlanInput,
    CreatePriceObservationsPlanInput, InitializePlanInput, PlannedAccount, PoolContext,
    RemoveLiquidityPlanInput, SwapExactInputPlanInput, SwapExactOutputPlanInput,
    SyncReservesPlanInput, TransactionPlan, UpdateConfigPlanInput,
};
pub use quote::AccountSnapshot;
pub use slippage::{
    maximum_guard_amount, minimum_guard_amount, prepare_add_liquidity, prepare_create_pool,
    prepare_remove_liquidity, prepare_swap_exact_input, prepare_swap_exact_output,
    PreparedAddLiquidity, PreparedCreatePool, PreparedRemoveLiquidity, PreparedSwapExactInput,
    PreparedSwapExactOutput, SlippageTolerance, SLIPPAGE_BPS_DENOMINATOR,
};
pub use transaction::{
    ensure_quote_unchanged, prepare_add_liquidity_transaction, prepare_create_pool_transaction,
    prepare_remove_liquidity_transaction, prepare_swap_exact_input_transaction,
    prepare_swap_exact_output_transaction, AddLiquidityTransactionInput, CallerAmounts,
    CreatePoolTransactionInput, FundingRequirement, PoolAccountSnapshots, PreparedTransaction,
    QuoteCommitment, RemoveLiquidityTransactionInput, SwapExactInputTransactionInput,
    SwapExactOutputTransactionInput, TransactionError, TransactionOperation, WalletPrerequisites,
};
