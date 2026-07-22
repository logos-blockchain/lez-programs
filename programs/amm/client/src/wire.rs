//! Lossless JSON transport adapters for the typed AMM client API.

use std::{error::Error, fmt, str::FromStr};

use amm_core::{
    AmmConfig, PoolDefinition, FEE_BPS_DENOMINATOR, MINIMUM_LIQUIDITY, SUPPORTED_FEE_TIERS,
};
use amm_program::quote::{
    AddLiquidityQuote, CreatePoolQuote, OraclePriceAccountQuote, PairOrder, PoolUpdate,
    RemoveLiquidityQuote, SwapDirection, SwapQuote, SyncReservesQuote,
};
use nssa_core::{
    account::{Account, AccountId, Data, Nonce},
    program::ProgramId,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    plan_add_liquidity, plan_create_oracle_price_account, plan_create_pool,
    plan_create_price_observations, plan_initialize, plan_remove_liquidity, plan_swap_exact_input,
    plan_swap_exact_output, plan_sync_reserves, plan_update_config,
    quote::{
        self as client_quote, AccountSnapshot, ValidatedFungibleDefinition,
        ValidatedFungibleHolding, ValidatedPoolSnapshot,
    },
    AddLiquidityPlanInput, AmmContext, ClientError, CreateOraclePriceAccountPlanInput,
    CreatePoolPlanInput, CreatePriceObservationsPlanInput, InitializePlanInput, PoolContext,
    PreparedAddLiquidity, PreparedCreatePool, PreparedRemoveLiquidity, PreparedSwapExactInput,
    PreparedSwapExactOutput, RemoveLiquidityPlanInput, SlippageTolerance, SwapExactInputPlanInput,
    SwapExactOutputPlanInput, SyncReservesPlanInput, TransactionPlan, UpdateConfigPlanInput,
    SLIPPAGE_BPS_DENOMINATOR,
};

/// Stable transport failure returned by the JSON and C ABI adapters.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WireError {
    code: String,
    message: String,
}

impl WireError {
    fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    /// Stable machine-readable error code.
    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }
}

impl fmt::Display for WireError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for WireError {}

impl From<ClientError> for WireError {
    fn from(error: ClientError) -> Self {
        Self::new(error.code(), error.to_string())
    }
}

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
enum PlanRequest {
    Initialize {
        #[serde(rename = "ammProgramId")]
        amm_program_id: ProgramId,
        #[serde(rename = "tokenProgramId")]
        token_program_id: ProgramId,
        #[serde(rename = "twapOracleProgramId")]
        twap_oracle_program_id: ProgramId,
        authority: String,
    },
    UpdateConfig {
        context: ContextInput,
        #[serde(rename = "tokenProgramId")]
        token_program_id: Option<ProgramId>,
        #[serde(rename = "twapOracleProgramId")]
        twap_oracle_program_id: Option<ProgramId>,
        #[serde(rename = "newAuthority")]
        new_authority: Option<String>,
    },
    CreatePriceObservations {
        context: ContextInput,
        #[serde(rename = "poolId")]
        pool_id: String,
        #[serde(rename = "windowDuration")]
        window_duration: String,
    },
    CreateOraclePriceAccount {
        context: ContextInput,
        #[serde(rename = "poolId")]
        pool_id: String,
        #[serde(rename = "windowDuration")]
        window_duration: String,
    },
    CreatePool {
        context: ContextInput,
        #[serde(rename = "tokenADefinitionId")]
        token_a_definition_id: String,
        #[serde(rename = "tokenBDefinitionId")]
        token_b_definition_id: String,
        #[serde(rename = "userHoldingA")]
        user_holding_a: String,
        #[serde(rename = "userHoldingB")]
        user_holding_b: String,
        #[serde(rename = "userHoldingLp")]
        user_holding_lp: String,
        #[serde(rename = "tokenAAmount")]
        token_a_amount: String,
        #[serde(rename = "tokenBAmount")]
        token_b_amount: String,
        fees: String,
        deadline: String,
    },
    AddLiquidity {
        context: ContextInput,
        pool: PoolInput,
        #[serde(rename = "userHoldingA")]
        user_holding_a: String,
        #[serde(rename = "userHoldingB")]
        user_holding_b: String,
        #[serde(rename = "userHoldingLp")]
        user_holding_lp: String,
        #[serde(rename = "minAmountLiquidity")]
        min_amount_liquidity: String,
        #[serde(rename = "maxAmountToAddTokenA")]
        max_amount_to_add_token_a: String,
        #[serde(rename = "maxAmountToAddTokenB")]
        max_amount_to_add_token_b: String,
        deadline: String,
    },
    RemoveLiquidity {
        context: ContextInput,
        pool: PoolInput,
        #[serde(rename = "userHoldingA")]
        user_holding_a: String,
        #[serde(rename = "userHoldingB")]
        user_holding_b: String,
        #[serde(rename = "userHoldingLp")]
        user_holding_lp: String,
        #[serde(rename = "removeLiquidityAmount")]
        remove_liquidity_amount: String,
        #[serde(rename = "minAmountToRemoveTokenA")]
        min_amount_to_remove_token_a: String,
        #[serde(rename = "minAmountToRemoveTokenB")]
        min_amount_to_remove_token_b: String,
        deadline: String,
    },
    SwapExactInput {
        context: ContextInput,
        pool: PoolInput,
        #[serde(rename = "userInputHolding")]
        user_input_holding: String,
        #[serde(rename = "userOutputHolding")]
        user_output_holding: String,
        #[serde(rename = "swapAmountIn")]
        swap_amount_in: String,
        #[serde(rename = "minAmountOut")]
        min_amount_out: String,
        deadline: String,
    },
    SwapExactOutput {
        context: ContextInput,
        pool: PoolInput,
        #[serde(rename = "userInputHolding")]
        user_input_holding: String,
        #[serde(rename = "userOutputHolding")]
        user_output_holding: String,
        #[serde(rename = "exactAmountOut")]
        exact_amount_out: String,
        #[serde(rename = "maxAmountIn")]
        max_amount_in: String,
        deadline: String,
    },
    SyncReserves {
        context: ContextInput,
        pool: PoolInput,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContextInput {
    amm_program_id: ProgramId,
    token_program_id: ProgramId,
    twap_oracle_program_id: ProgramId,
    authority: String,
}

impl ContextInput {
    fn into_context(self) -> Result<AmmContext, WireError> {
        Ok(AmmContext::new(
            self.amm_program_id,
            AmmConfig {
                token_program_id: self.token_program_id,
                twap_oracle_program_id: self.twap_oracle_program_id,
                authority: account_id(&self.authority, "context.authority")?,
            },
        ))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PoolInput {
    pool_id: String,
    definition_token_a_id: String,
    definition_token_b_id: String,
    vault_a_id: String,
    vault_b_id: String,
    liquidity_pool_id: String,
    liquidity_pool_supply: String,
    reserve_a: String,
    reserve_b: String,
    fees: String,
}

impl PoolInput {
    fn into_pool(self) -> Result<(AccountId, PoolDefinition), WireError> {
        Ok((
            account_id(&self.pool_id, "pool.poolId")?,
            PoolDefinition {
                definition_token_a_id: account_id(
                    &self.definition_token_a_id,
                    "pool.definitionTokenAId",
                )?,
                definition_token_b_id: account_id(
                    &self.definition_token_b_id,
                    "pool.definitionTokenBId",
                )?,
                vault_a_id: account_id(&self.vault_a_id, "pool.vaultAId")?,
                vault_b_id: account_id(&self.vault_b_id, "pool.vaultBId")?,
                liquidity_pool_id: account_id(&self.liquidity_pool_id, "pool.liquidityPoolId")?,
                liquidity_pool_supply: decimal_u128(
                    &self.liquidity_pool_supply,
                    "pool.liquidityPoolSupply",
                )?,
                reserve_a: decimal_u128(&self.reserve_a, "pool.reserveA")?,
                reserve_b: decimal_u128(&self.reserve_b, "pool.reserveB")?,
                fees: decimal_u128(&self.fees, "pool.fees")?,
            },
        ))
    }
}

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
enum QuoteRequest {
    ProtocolConstants,
    PairOrder {
        #[serde(flatten)]
        state: PoolStateInput,
        #[serde(rename = "firstTokenDefinitionId")]
        first_token_definition_id: String,
        #[serde(rename = "secondTokenDefinitionId")]
        second_token_definition_id: String,
    },
    CreatePool {
        #[serde(rename = "ammProgramId")]
        amm_program_id: ProgramId,
        config: AccountSnapshotInput,
        #[serde(rename = "tokenADefinition")]
        token_a_definition: AccountSnapshotInput,
        #[serde(rename = "tokenBDefinition")]
        token_b_definition: AccountSnapshotInput,
        #[serde(rename = "tokenAAmount")]
        token_a_amount: String,
        #[serde(rename = "tokenBAmount")]
        token_b_amount: String,
        #[serde(rename = "feeBps")]
        fee_bps: String,
    },
    PrepareCreatePool {
        #[serde(rename = "ammProgramId")]
        amm_program_id: ProgramId,
        config: AccountSnapshotInput,
        #[serde(rename = "tokenADefinition")]
        token_a_definition: AccountSnapshotInput,
        #[serde(rename = "tokenBDefinition")]
        token_b_definition: AccountSnapshotInput,
        #[serde(rename = "tokenAAmount")]
        token_a_amount: String,
        #[serde(rename = "tokenBAmount")]
        token_b_amount: String,
        #[serde(rename = "feeBps")]
        fee_bps: String,
    },
    PreviewAddLiquidity {
        #[serde(flatten)]
        state: PoolStateInput,
        #[serde(rename = "maxAmountA")]
        max_amount_a: String,
        #[serde(rename = "maxAmountB")]
        max_amount_b: String,
    },
    PrepareAddLiquidity {
        #[serde(flatten)]
        state: PoolStateInput,
        #[serde(rename = "maxAmountA")]
        max_amount_a: String,
        #[serde(rename = "maxAmountB")]
        max_amount_b: String,
        #[serde(rename = "slippageBps")]
        slippage_bps: String,
    },
    AddLiquidity {
        #[serde(flatten)]
        state: PoolStateInput,
        #[serde(rename = "maxAmountA")]
        max_amount_a: String,
        #[serde(rename = "maxAmountB")]
        max_amount_b: String,
        #[serde(rename = "minimumLiquidity")]
        minimum_liquidity: String,
    },
    PreviewRemoveLiquidity {
        #[serde(flatten)]
        state: PoolStateInput,
        #[serde(rename = "userLiquidityHolding")]
        user_liquidity_holding: AccountSnapshotInput,
        #[serde(rename = "removeLiquidityAmount")]
        remove_liquidity_amount: String,
    },
    PrepareRemoveLiquidity {
        #[serde(flatten)]
        state: PoolStateInput,
        #[serde(rename = "userLiquidityHolding")]
        user_liquidity_holding: AccountSnapshotInput,
        #[serde(rename = "removeLiquidityAmount")]
        remove_liquidity_amount: String,
        #[serde(rename = "slippageBps")]
        slippage_bps: String,
    },
    RemoveLiquidity {
        #[serde(flatten)]
        state: PoolStateInput,
        #[serde(rename = "userLiquidityHolding")]
        user_liquidity_holding: AccountSnapshotInput,
        #[serde(rename = "removeLiquidityAmount")]
        remove_liquidity_amount: String,
        #[serde(rename = "minimumAmountA")]
        minimum_amount_a: String,
        #[serde(rename = "minimumAmountB")]
        minimum_amount_b: String,
    },
    PreviewSwapExactInput {
        #[serde(flatten)]
        state: PoolStateInput,
        #[serde(rename = "userInputHolding")]
        user_input_holding: AccountSnapshotInput,
        #[serde(rename = "userOutputHolding")]
        user_output_holding: AccountSnapshotInput,
        #[serde(rename = "inputTokenDefinitionId")]
        input_token_definition_id: String,
        #[serde(rename = "amountIn")]
        amount_in: String,
    },
    PrepareSwapExactInput {
        #[serde(flatten)]
        state: PoolStateInput,
        #[serde(rename = "userInputHolding")]
        user_input_holding: AccountSnapshotInput,
        #[serde(rename = "userOutputHolding")]
        user_output_holding: AccountSnapshotInput,
        #[serde(rename = "inputTokenDefinitionId")]
        input_token_definition_id: String,
        #[serde(rename = "amountIn")]
        amount_in: String,
        #[serde(rename = "slippageBps")]
        slippage_bps: String,
    },
    SwapExactInput {
        #[serde(flatten)]
        state: PoolStateInput,
        #[serde(rename = "userInputHolding")]
        user_input_holding: AccountSnapshotInput,
        #[serde(rename = "userOutputHolding")]
        user_output_holding: AccountSnapshotInput,
        #[serde(rename = "inputTokenDefinitionId")]
        input_token_definition_id: String,
        #[serde(rename = "amountIn")]
        amount_in: String,
        #[serde(rename = "minimumAmountOut")]
        minimum_amount_out: String,
    },
    PreviewSwapExactOutput {
        #[serde(flatten)]
        state: PoolStateInput,
        #[serde(rename = "userInputHolding")]
        user_input_holding: AccountSnapshotInput,
        #[serde(rename = "userOutputHolding")]
        user_output_holding: AccountSnapshotInput,
        #[serde(rename = "inputTokenDefinitionId")]
        input_token_definition_id: String,
        #[serde(rename = "exactAmountOut")]
        exact_amount_out: String,
    },
    PrepareSwapExactOutput {
        #[serde(flatten)]
        state: PoolStateInput,
        #[serde(rename = "userInputHolding")]
        user_input_holding: AccountSnapshotInput,
        #[serde(rename = "userOutputHolding")]
        user_output_holding: AccountSnapshotInput,
        #[serde(rename = "inputTokenDefinitionId")]
        input_token_definition_id: String,
        #[serde(rename = "exactAmountOut")]
        exact_amount_out: String,
        #[serde(rename = "slippageBps")]
        slippage_bps: String,
    },
    SwapExactOutput {
        #[serde(flatten)]
        state: PoolStateInput,
        #[serde(rename = "userInputHolding")]
        user_input_holding: AccountSnapshotInput,
        #[serde(rename = "userOutputHolding")]
        user_output_holding: AccountSnapshotInput,
        #[serde(rename = "inputTokenDefinitionId")]
        input_token_definition_id: String,
        #[serde(rename = "exactAmountOut")]
        exact_amount_out: String,
        #[serde(rename = "maximumAmountIn")]
        maximum_amount_in: String,
    },
    SyncReserves {
        #[serde(flatten)]
        state: PoolStateInput,
    },
    CreateOraclePriceAccount {
        #[serde(flatten)]
        state: PoolStateInput,
        #[serde(rename = "windowDuration")]
        window_duration: String,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PoolStateInput {
    amm_program_id: ProgramId,
    config: AccountSnapshotInput,
    snapshot: PoolSnapshotInput,
}

impl PoolStateInput {
    fn validate(self) -> Result<(AmmContext, ValidatedPoolSnapshot), WireError> {
        let config = self.config.into_snapshot()?;
        let context = AmmContext::from_config_account(self.amm_program_id, &config)?;
        let snapshot = self.snapshot.validate(&context)?;
        Ok((context, snapshot))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PoolSnapshotInput {
    pool: AccountSnapshotInput,
    token_a_definition: AccountSnapshotInput,
    token_b_definition: AccountSnapshotInput,
    vault_a: AccountSnapshotInput,
    vault_b: AccountSnapshotInput,
    liquidity_definition: AccountSnapshotInput,
}

impl PoolSnapshotInput {
    fn validate(self, context: &AmmContext) -> Result<ValidatedPoolSnapshot, WireError> {
        let pool = self.pool.into_snapshot()?;
        let token_a_definition = self.token_a_definition.into_snapshot()?;
        let token_b_definition = self.token_b_definition.into_snapshot()?;
        let vault_a = self.vault_a.into_snapshot()?;
        let vault_b = self.vault_b.into_snapshot()?;
        let liquidity_definition = self.liquidity_definition.into_snapshot()?;

        Ok(ValidatedPoolSnapshot::new(
            context,
            &pool,
            &token_a_definition,
            &token_b_definition,
            &vault_a,
            &vault_b,
            &liquidity_definition,
        )?)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountSnapshotInput {
    id: String,
    program_owner: ProgramId,
    balance: String,
    nonce: String,
    data: String,
}

impl AccountSnapshotInput {
    fn into_snapshot(self) -> Result<AccountSnapshot, WireError> {
        let bytes = hex_bytes(&self.data, "account.data")?;
        let data = Data::try_from(bytes)
            .map_err(|error| invalid_request(format!("account.data is too large: {error}")))?;
        Ok(AccountSnapshot::new(
            account_id(&self.id, "account.id")?,
            Account {
                program_owner: self.program_owner,
                balance: decimal_u128(&self.balance, "account.balance")?,
                data,
                nonce: Nonce(decimal_u128(&self.nonce, "account.nonce")?),
            },
        ))
    }
}

/// Builds one of the ten canonical transaction plans from tagged JSON.
pub fn plan_json(value: Value) -> Result<Value, WireError> {
    let request: PlanRequest = serde_json::from_value(value)
        .map_err(|error| invalid_request(format!("invalid plan request: {error}")))?;
    let plan = match request {
        PlanRequest::Initialize {
            amm_program_id,
            token_program_id,
            twap_oracle_program_id,
            authority,
        } => plan_initialize(InitializePlanInput {
            amm_program_id,
            token_program_id,
            twap_oracle_program_id,
            authority: account_id(&authority, "authority")?,
        }),
        PlanRequest::UpdateConfig {
            context,
            token_program_id,
            twap_oracle_program_id,
            new_authority,
        } => {
            let context = context.into_context()?;
            let new_authority = new_authority
                .as_deref()
                .map(|value| account_id(value, "newAuthority"))
                .transpose()?;
            plan_update_config(UpdateConfigPlanInput {
                context: &context,
                token_program_id,
                twap_oracle_program_id,
                new_authority,
            })
        }
        PlanRequest::CreatePriceObservations {
            context,
            pool_id,
            window_duration,
        } => {
            let context = context.into_context()?;
            plan_create_price_observations(CreatePriceObservationsPlanInput {
                context: &context,
                pool_id: account_id(&pool_id, "poolId")?,
                window_duration: decimal_u64(&window_duration, "windowDuration")?,
            })
        }
        PlanRequest::CreateOraclePriceAccount {
            context,
            pool_id,
            window_duration,
        } => {
            let context = context.into_context()?;
            plan_create_oracle_price_account(CreateOraclePriceAccountPlanInput {
                context: &context,
                pool_id: account_id(&pool_id, "poolId")?,
                window_duration: decimal_u64(&window_duration, "windowDuration")?,
            })
        }
        PlanRequest::CreatePool {
            context,
            token_a_definition_id,
            token_b_definition_id,
            user_holding_a,
            user_holding_b,
            user_holding_lp,
            token_a_amount,
            token_b_amount,
            fees,
            deadline,
        } => {
            let context = context.into_context()?;
            plan_create_pool(CreatePoolPlanInput {
                context: &context,
                token_a_definition_id: account_id(&token_a_definition_id, "tokenADefinitionId")?,
                token_b_definition_id: account_id(&token_b_definition_id, "tokenBDefinitionId")?,
                user_holding_a: account_id(&user_holding_a, "userHoldingA")?,
                user_holding_b: account_id(&user_holding_b, "userHoldingB")?,
                user_holding_lp: account_id(&user_holding_lp, "userHoldingLp")?,
                token_a_amount: decimal_u128(&token_a_amount, "tokenAAmount")?,
                token_b_amount: decimal_u128(&token_b_amount, "tokenBAmount")?,
                fees: decimal_u128(&fees, "fees")?,
                deadline: decimal_u64(&deadline, "deadline")?,
            })?
        }
        PlanRequest::AddLiquidity {
            context,
            pool,
            user_holding_a,
            user_holding_b,
            user_holding_lp,
            min_amount_liquidity,
            max_amount_to_add_token_a,
            max_amount_to_add_token_b,
            deadline,
        } => {
            let context = context.into_context()?;
            let (pool_id, pool) = pool.into_pool()?;
            plan_add_liquidity(AddLiquidityPlanInput {
                context: &context,
                pool: PoolContext::new(&context, pool_id, &pool)?,
                user_holding_a: account_id(&user_holding_a, "userHoldingA")?,
                user_holding_b: account_id(&user_holding_b, "userHoldingB")?,
                user_holding_lp: account_id(&user_holding_lp, "userHoldingLp")?,
                min_amount_liquidity: decimal_u128(&min_amount_liquidity, "minAmountLiquidity")?,
                max_amount_to_add_token_a: decimal_u128(
                    &max_amount_to_add_token_a,
                    "maxAmountToAddTokenA",
                )?,
                max_amount_to_add_token_b: decimal_u128(
                    &max_amount_to_add_token_b,
                    "maxAmountToAddTokenB",
                )?,
                deadline: decimal_u64(&deadline, "deadline")?,
            })
        }
        PlanRequest::RemoveLiquidity {
            context,
            pool,
            user_holding_a,
            user_holding_b,
            user_holding_lp,
            remove_liquidity_amount,
            min_amount_to_remove_token_a,
            min_amount_to_remove_token_b,
            deadline,
        } => {
            let context = context.into_context()?;
            let (pool_id, pool) = pool.into_pool()?;
            plan_remove_liquidity(RemoveLiquidityPlanInput {
                context: &context,
                pool: PoolContext::new(&context, pool_id, &pool)?,
                user_holding_a: account_id(&user_holding_a, "userHoldingA")?,
                user_holding_b: account_id(&user_holding_b, "userHoldingB")?,
                user_holding_lp: account_id(&user_holding_lp, "userHoldingLp")?,
                remove_liquidity_amount: decimal_u128(
                    &remove_liquidity_amount,
                    "removeLiquidityAmount",
                )?,
                min_amount_to_remove_token_a: decimal_u128(
                    &min_amount_to_remove_token_a,
                    "minAmountToRemoveTokenA",
                )?,
                min_amount_to_remove_token_b: decimal_u128(
                    &min_amount_to_remove_token_b,
                    "minAmountToRemoveTokenB",
                )?,
                deadline: decimal_u64(&deadline, "deadline")?,
            })
        }
        PlanRequest::SwapExactInput {
            context,
            pool,
            user_input_holding,
            user_output_holding,
            swap_amount_in,
            min_amount_out,
            deadline,
        } => {
            let context = context.into_context()?;
            let (pool_id, pool) = pool.into_pool()?;
            plan_swap_exact_input(SwapExactInputPlanInput {
                context: &context,
                pool: PoolContext::new(&context, pool_id, &pool)?,
                user_input_holding: account_id(&user_input_holding, "userInputHolding")?,
                user_output_holding: account_id(&user_output_holding, "userOutputHolding")?,
                swap_amount_in: decimal_u128(&swap_amount_in, "swapAmountIn")?,
                min_amount_out: decimal_u128(&min_amount_out, "minAmountOut")?,
                deadline: decimal_u64(&deadline, "deadline")?,
            })
        }
        PlanRequest::SwapExactOutput {
            context,
            pool,
            user_input_holding,
            user_output_holding,
            exact_amount_out,
            max_amount_in,
            deadline,
        } => {
            let context = context.into_context()?;
            let (pool_id, pool) = pool.into_pool()?;
            plan_swap_exact_output(SwapExactOutputPlanInput {
                context: &context,
                pool: PoolContext::new(&context, pool_id, &pool)?,
                user_input_holding: account_id(&user_input_holding, "userInputHolding")?,
                user_output_holding: account_id(&user_output_holding, "userOutputHolding")?,
                exact_amount_out: decimal_u128(&exact_amount_out, "exactAmountOut")?,
                max_amount_in: decimal_u128(&max_amount_in, "maxAmountIn")?,
                deadline: decimal_u64(&deadline, "deadline")?,
            })
        }
        PlanRequest::SyncReserves { context, pool } => {
            let context = context.into_context()?;
            let (pool_id, pool) = pool.into_pool()?;
            plan_sync_reserves(SyncReservesPlanInput {
                context: &context,
                pool: PoolContext::new(&context, pool_id, &pool)?,
            })
        }
    };

    transaction_plan_json(&plan)
}

/// Evaluates one reusable AMM economic quote from tagged JSON.
pub fn quote_json(value: Value) -> Result<Value, WireError> {
    let request: QuoteRequest = serde_json::from_value(value)
        .map_err(|error| invalid_request(format!("invalid quote request: {error}")))?;
    match request {
        QuoteRequest::ProtocolConstants => Ok(json!({
            "minimumLiquidity": MINIMUM_LIQUIDITY.to_string(),
            "feeBpsDenominator": FEE_BPS_DENOMINATOR.to_string(),
            "slippageBpsDenominator": SLIPPAGE_BPS_DENOMINATOR.to_string(),
            "supportedFeeTiers": SUPPORTED_FEE_TIERS
                .iter()
                .map(u128::to_string)
                .collect::<Vec<_>>(),
        })),
        QuoteRequest::PairOrder {
            state,
            first_token_definition_id,
            second_token_definition_id,
        } => {
            let (_, snapshot) = state.validate()?;
            let first_id = account_id(&first_token_definition_id, "firstTokenDefinitionId")?;
            let second_id = account_id(&second_token_definition_id, "secondTokenDefinitionId")?;
            let first = pool_definition(&snapshot, first_id, "firstTokenDefinitionId")?;
            let second = pool_definition(&snapshot, second_id, "secondTokenDefinitionId")?;
            let order = client_quote::pair_order(&snapshot, first, second)?;
            Ok(json!({
                "order": match order {
                    PairOrder::Stored => "stored",
                    PairOrder::Reversed => "reversed",
                },
            }))
        }
        QuoteRequest::CreatePool {
            amm_program_id,
            config,
            token_a_definition,
            token_b_definition,
            token_a_amount,
            token_b_amount,
            fee_bps,
        } => {
            let config = config.into_snapshot()?;
            let context = AmmContext::from_config_account(amm_program_id, &config)?;
            let token_a_definition = token_a_definition.into_snapshot()?;
            let token_b_definition = token_b_definition.into_snapshot()?;
            let token_a = ValidatedFungibleDefinition::new(&context, &token_a_definition)?;
            let token_b = ValidatedFungibleDefinition::new(&context, &token_b_definition)?;
            let quote = client_quote::create_pool(
                &context,
                &token_a,
                &token_b,
                decimal_u128(&token_a_amount, "tokenAAmount")?,
                decimal_u128(&token_b_amount, "tokenBAmount")?,
                decimal_u128(&fee_bps, "feeBps")?,
            )?;
            Ok(create_pool_quote_json(quote))
        }
        QuoteRequest::PrepareCreatePool {
            amm_program_id,
            config,
            token_a_definition,
            token_b_definition,
            token_a_amount,
            token_b_amount,
            fee_bps,
        } => {
            let config = config.into_snapshot()?;
            let context = AmmContext::from_config_account(amm_program_id, &config)?;
            let token_a_definition = token_a_definition.into_snapshot()?;
            let token_b_definition = token_b_definition.into_snapshot()?;
            let token_a = ValidatedFungibleDefinition::new(&context, &token_a_definition)?;
            let token_b = ValidatedFungibleDefinition::new(&context, &token_b_definition)?;
            let prepared = crate::prepare_create_pool(
                &context,
                &token_a,
                &token_b,
                decimal_u128(&token_a_amount, "tokenAAmount")?,
                decimal_u128(&token_b_amount, "tokenBAmount")?,
                decimal_u128(&fee_bps, "feeBps")?,
            )?;
            Ok(prepared_create_pool_json(prepared))
        }
        QuoteRequest::PreviewAddLiquidity {
            state,
            max_amount_a,
            max_amount_b,
        } => {
            let (_, snapshot) = state.validate()?;
            let quote = client_quote::preview_add_liquidity(
                &snapshot,
                decimal_u128(&max_amount_a, "maxAmountA")?,
                decimal_u128(&max_amount_b, "maxAmountB")?,
            )?;
            Ok(add_liquidity_quote_json(quote))
        }
        QuoteRequest::PrepareAddLiquidity {
            state,
            max_amount_a,
            max_amount_b,
            slippage_bps,
        } => {
            let (_, snapshot) = state.validate()?;
            let prepared = crate::prepare_add_liquidity(
                &snapshot,
                decimal_u128(&max_amount_a, "maxAmountA")?,
                decimal_u128(&max_amount_b, "maxAmountB")?,
                slippage_tolerance(&slippage_bps)?,
            )?;
            Ok(prepared_add_liquidity_json(prepared))
        }
        QuoteRequest::AddLiquidity {
            state,
            max_amount_a,
            max_amount_b,
            minimum_liquidity,
        } => {
            let (_, snapshot) = state.validate()?;
            let quote = client_quote::add_liquidity(
                &snapshot,
                decimal_u128(&max_amount_a, "maxAmountA")?,
                decimal_u128(&max_amount_b, "maxAmountB")?,
                decimal_u128(&minimum_liquidity, "minimumLiquidity")?,
            )?;
            Ok(add_liquidity_quote_json(quote))
        }
        QuoteRequest::PreviewRemoveLiquidity {
            state,
            user_liquidity_holding,
            remove_liquidity_amount,
        } => {
            let (context, snapshot) = state.validate()?;
            let user_liquidity = validated_holding(
                &context,
                user_liquidity_holding,
                snapshot.liquidity_definition(),
            )?;
            let quote = client_quote::preview_remove_liquidity(
                &snapshot,
                &user_liquidity,
                decimal_u128(&remove_liquidity_amount, "removeLiquidityAmount")?,
            )?;
            Ok(remove_liquidity_quote_json(quote))
        }
        QuoteRequest::PrepareRemoveLiquidity {
            state,
            user_liquidity_holding,
            remove_liquidity_amount,
            slippage_bps,
        } => {
            let (context, snapshot) = state.validate()?;
            let user_liquidity = validated_holding(
                &context,
                user_liquidity_holding,
                snapshot.liquidity_definition(),
            )?;
            let prepared = crate::prepare_remove_liquidity(
                &snapshot,
                &user_liquidity,
                decimal_u128(&remove_liquidity_amount, "removeLiquidityAmount")?,
                slippage_tolerance(&slippage_bps)?,
            )?;
            Ok(prepared_remove_liquidity_json(prepared))
        }
        QuoteRequest::RemoveLiquidity {
            state,
            user_liquidity_holding,
            remove_liquidity_amount,
            minimum_amount_a,
            minimum_amount_b,
        } => {
            let (context, snapshot) = state.validate()?;
            let user_liquidity = validated_holding(
                &context,
                user_liquidity_holding,
                snapshot.liquidity_definition(),
            )?;
            let quote = client_quote::remove_liquidity(
                &snapshot,
                &user_liquidity,
                decimal_u128(&remove_liquidity_amount, "removeLiquidityAmount")?,
                decimal_u128(&minimum_amount_a, "minimumAmountA")?,
                decimal_u128(&minimum_amount_b, "minimumAmountB")?,
            )?;
            Ok(remove_liquidity_quote_json(quote))
        }
        QuoteRequest::PreviewSwapExactInput {
            state,
            user_input_holding,
            user_output_holding,
            input_token_definition_id,
            amount_in,
        } => {
            let (context, snapshot) = state.validate()?;
            let (user_input, user_output) = validated_swap_holdings(
                &context,
                &snapshot,
                user_input_holding,
                user_output_holding,
                &input_token_definition_id,
            )?;
            let quote = client_quote::preview_swap_exact_input(
                &snapshot,
                &user_input,
                &user_output,
                decimal_u128(&amount_in, "amountIn")?,
            )?;
            Ok(swap_quote_json(quote))
        }
        QuoteRequest::PrepareSwapExactInput {
            state,
            user_input_holding,
            user_output_holding,
            input_token_definition_id,
            amount_in,
            slippage_bps,
        } => {
            let (context, snapshot) = state.validate()?;
            let (user_input, user_output) = validated_swap_holdings(
                &context,
                &snapshot,
                user_input_holding,
                user_output_holding,
                &input_token_definition_id,
            )?;
            let prepared = crate::prepare_swap_exact_input(
                &snapshot,
                &user_input,
                &user_output,
                decimal_u128(&amount_in, "amountIn")?,
                slippage_tolerance(&slippage_bps)?,
            )?;
            Ok(prepared_swap_exact_input_json(prepared))
        }
        QuoteRequest::SwapExactInput {
            state,
            user_input_holding,
            user_output_holding,
            input_token_definition_id,
            amount_in,
            minimum_amount_out,
        } => {
            let (context, snapshot) = state.validate()?;
            let (user_input, user_output) = validated_swap_holdings(
                &context,
                &snapshot,
                user_input_holding,
                user_output_holding,
                &input_token_definition_id,
            )?;
            let quote = client_quote::swap_exact_input(
                &snapshot,
                &user_input,
                &user_output,
                decimal_u128(&amount_in, "amountIn")?,
                decimal_u128(&minimum_amount_out, "minimumAmountOut")?,
            )?;
            Ok(swap_quote_json(quote))
        }
        QuoteRequest::PreviewSwapExactOutput {
            state,
            user_input_holding,
            user_output_holding,
            input_token_definition_id,
            exact_amount_out,
        } => {
            let (context, snapshot) = state.validate()?;
            let (user_input, user_output) = validated_swap_holdings(
                &context,
                &snapshot,
                user_input_holding,
                user_output_holding,
                &input_token_definition_id,
            )?;
            let quote = client_quote::preview_swap_exact_output(
                &snapshot,
                &user_input,
                &user_output,
                decimal_u128(&exact_amount_out, "exactAmountOut")?,
            )?;
            Ok(swap_quote_json(quote))
        }
        QuoteRequest::PrepareSwapExactOutput {
            state,
            user_input_holding,
            user_output_holding,
            input_token_definition_id,
            exact_amount_out,
            slippage_bps,
        } => {
            let (context, snapshot) = state.validate()?;
            let (user_input, user_output) = validated_swap_holdings(
                &context,
                &snapshot,
                user_input_holding,
                user_output_holding,
                &input_token_definition_id,
            )?;
            let prepared = crate::prepare_swap_exact_output(
                &snapshot,
                &user_input,
                &user_output,
                decimal_u128(&exact_amount_out, "exactAmountOut")?,
                slippage_tolerance(&slippage_bps)?,
            )?;
            Ok(prepared_swap_exact_output_json(prepared))
        }
        QuoteRequest::SwapExactOutput {
            state,
            user_input_holding,
            user_output_holding,
            input_token_definition_id,
            exact_amount_out,
            maximum_amount_in,
        } => {
            let (context, snapshot) = state.validate()?;
            let (user_input, user_output) = validated_swap_holdings(
                &context,
                &snapshot,
                user_input_holding,
                user_output_holding,
                &input_token_definition_id,
            )?;
            let quote = client_quote::swap_exact_output(
                &snapshot,
                &user_input,
                &user_output,
                decimal_u128(&exact_amount_out, "exactAmountOut")?,
                decimal_u128(&maximum_amount_in, "maximumAmountIn")?,
            )?;
            Ok(swap_quote_json(quote))
        }
        QuoteRequest::SyncReserves { state } => {
            let (_, snapshot) = state.validate()?;
            Ok(sync_reserves_quote_json(client_quote::sync_reserves(
                &snapshot,
            )?))
        }
        QuoteRequest::CreateOraclePriceAccount {
            state,
            window_duration,
        } => {
            let (_, snapshot) = state.validate()?;
            Ok(oracle_price_quote_json(
                client_quote::create_oracle_price_account(
                    &snapshot,
                    decimal_u64(&window_duration, "windowDuration")?,
                )?,
            ))
        }
    }
}

fn transaction_plan_json(plan: &TransactionPlan) -> Result<Value, WireError> {
    let instruction_words = plan.instruction_data().map_err(|error| {
        WireError::new(
            "instruction_encoding_failed",
            format!("instruction serialization failed: {error}"),
        )
    })?;
    let accounts = plan
        .accounts()
        .iter()
        .map(|account| {
            json!({
                "id": account.id().to_string(),
                "role": account.role().as_str(),
                "writable": account.writable(),
                "signer": account.signer(),
                "init": account.init(),
            })
        })
        .collect::<Vec<_>>();

    Ok(json!({
        "instruction": plan.instruction_name(),
        "programId": plan.program_id(),
        "accounts": accounts,
        "instructionWords": instruction_words,
    }))
}

fn pool_definition<'a>(
    snapshot: &'a ValidatedPoolSnapshot,
    definition_id: AccountId,
    field: &str,
) -> Result<&'a ValidatedFungibleDefinition, WireError> {
    if snapshot.token_a_definition().account_id() == definition_id {
        Ok(snapshot.token_a_definition())
    } else if snapshot.token_b_definition().account_id() == definition_id {
        Ok(snapshot.token_b_definition())
    } else {
        Err(invalid_request(format!(
            "{field} is not one of the pool token definitions"
        )))
    }
}

fn validated_holding(
    context: &AmmContext,
    holding: AccountSnapshotInput,
    definition: &ValidatedFungibleDefinition,
) -> Result<ValidatedFungibleHolding, WireError> {
    let holding = holding.into_snapshot()?;
    Ok(ValidatedFungibleHolding::new(
        context, &holding, definition,
    )?)
}

fn validated_swap_holdings(
    context: &AmmContext,
    snapshot: &ValidatedPoolSnapshot,
    input_holding: AccountSnapshotInput,
    output_holding: AccountSnapshotInput,
    definition_id: &str,
) -> Result<(ValidatedFungibleHolding, ValidatedFungibleHolding), WireError> {
    let definition_id = account_id(definition_id, "inputTokenDefinitionId")?;
    let input_definition = pool_definition(snapshot, definition_id, "inputTokenDefinitionId")?;
    let output_definition =
        if input_definition.account_id() == snapshot.token_a_definition().account_id() {
            snapshot.token_b_definition()
        } else {
            snapshot.token_a_definition()
        };
    Ok((
        validated_holding(context, input_holding, input_definition)?,
        validated_holding(context, output_holding, output_definition)?,
    ))
}

fn pool_update_json(pool: PoolUpdate) -> Value {
    json!({
        "liquidityPoolSupply": pool.liquidity_pool_supply.to_string(),
        "reserveA": pool.reserve_a.to_string(),
        "reserveB": pool.reserve_b.to_string(),
        "spotPriceQ64_64": pool.spot_price_q64_64.to_string(),
    })
}

fn create_pool_quote_json(quote: CreatePoolQuote) -> Value {
    json!({
        "pool": pool_update_json(quote.pool),
        "lockedLiquidity": quote.locked_liquidity.to_string(),
        "userLiquidity": quote.user_liquidity.to_string(),
    })
}

fn prepared_create_pool_json(prepared: PreparedCreatePool) -> Value {
    json!({
        "quote": create_pool_quote_json(prepared.quote),
        "instructionArgs": {
            "tokenAAmount": prepared.token_a_amount.to_string(),
            "tokenBAmount": prepared.token_b_amount.to_string(),
            "fees": prepared.fees.to_string(),
        },
    })
}

fn add_liquidity_quote_json(quote: AddLiquidityQuote) -> Value {
    json!({
        "actualAmountA": quote.actual_amount_a.to_string(),
        "actualAmountB": quote.actual_amount_b.to_string(),
        "liquidityToMint": quote.liquidity_to_mint.to_string(),
        "pool": pool_update_json(quote.pool),
    })
}

fn prepared_add_liquidity_json(prepared: PreparedAddLiquidity) -> Value {
    json!({
        "quote": add_liquidity_quote_json(prepared.quote),
        "instructionArgs": {
            "minAmountLiquidity": prepared.min_amount_liquidity.to_string(),
            "maxAmountToAddTokenA": prepared.max_amount_to_add_token_a.to_string(),
            "maxAmountToAddTokenB": prepared.max_amount_to_add_token_b.to_string(),
        },
    })
}

fn remove_liquidity_quote_json(quote: RemoveLiquidityQuote) -> Value {
    json!({
        "withdrawAmountA": quote.withdraw_amount_a.to_string(),
        "withdrawAmountB": quote.withdraw_amount_b.to_string(),
        "liquidityToBurn": quote.liquidity_to_burn.to_string(),
        "pool": pool_update_json(quote.pool),
    })
}

fn prepared_remove_liquidity_json(prepared: PreparedRemoveLiquidity) -> Value {
    json!({
        "quote": remove_liquidity_quote_json(prepared.quote),
        "instructionArgs": {
            "removeLiquidityAmount": prepared.remove_liquidity_amount.to_string(),
            "minAmountToRemoveTokenA": prepared.min_amount_to_remove_token_a.to_string(),
            "minAmountToRemoveTokenB": prepared.min_amount_to_remove_token_b.to_string(),
        },
    })
}

fn swap_quote_json(quote: SwapQuote) -> Value {
    json!({
        "direction": match quote.direction {
            SwapDirection::AToB => "a_to_b",
            SwapDirection::BToA => "b_to_a",
        },
        "amountIn": quote.amount_in.to_string(),
        "effectiveAmountIn": quote.effective_amount_in.to_string(),
        "feeAmount": quote.fee_amount.to_string(),
        "amountOut": quote.amount_out.to_string(),
        "pool": pool_update_json(quote.pool),
    })
}

fn prepared_swap_exact_input_json(prepared: PreparedSwapExactInput) -> Value {
    json!({
        "quote": swap_quote_json(prepared.quote),
        "instructionArgs": {
            "swapAmountIn": prepared.swap_amount_in.to_string(),
            "minAmountOut": prepared.min_amount_out.to_string(),
        },
    })
}

fn prepared_swap_exact_output_json(prepared: PreparedSwapExactOutput) -> Value {
    json!({
        "quote": swap_quote_json(prepared.quote),
        "instructionArgs": {
            "exactAmountOut": prepared.exact_amount_out.to_string(),
            "maxAmountIn": prepared.max_amount_in.to_string(),
        },
    })
}

fn sync_reserves_quote_json(quote: SyncReservesQuote) -> Value {
    json!({
        "donatedAmountA": quote.donated_amount_a.to_string(),
        "donatedAmountB": quote.donated_amount_b.to_string(),
        "pool": pool_update_json(quote.pool),
    })
}

fn oracle_price_quote_json(quote: OraclePriceAccountQuote) -> Value {
    json!({
        "baseAsset": quote.base_asset.to_string(),
        "quoteAsset": quote.quote_asset.to_string(),
        "initialPriceQ64_64": quote.initial_price_q64_64.to_string(),
        "windowDuration": quote.window_duration.to_string(),
    })
}

fn account_id(value: &str, field: &str) -> Result<AccountId, WireError> {
    AccountId::from_str(value)
        .map_err(|error| invalid_request(format!("{field} is not a valid account ID: {error}")))
}

fn decimal_u128(value: &str, field: &str) -> Result<u128, WireError> {
    decimal(value, field)
}

fn decimal_u64(value: &str, field: &str) -> Result<u64, WireError> {
    decimal(value, field)
}

fn slippage_tolerance(value: &str) -> Result<SlippageTolerance, WireError> {
    Ok(SlippageTolerance::new(decimal_u128(value, "slippageBps")?)?)
}

fn decimal<T>(value: &str, field: &str) -> Result<T, WireError>
where
    T: FromStr,
{
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid_request(format!(
            "{field} must be a non-empty unsigned decimal string"
        )));
    }
    value.parse().map_err(|_| {
        invalid_request(format!(
            "{field} is outside the supported unsigned integer range"
        ))
    })
}

fn hex_bytes(value: &str, field: &str) -> Result<Vec<u8>, WireError> {
    let mut bytes = Vec::new();
    let mut chunks = value.as_bytes().chunks_exact(2);
    for chunk in &mut chunks {
        let Some(high) = chunk.first().and_then(|byte| hex_nibble(*byte)) else {
            return Err(invalid_request(format!(
                "{field} must be an even-length hexadecimal string"
            )));
        };
        let Some(low) = chunk.get(1).and_then(|byte| hex_nibble(*byte)) else {
            return Err(invalid_request(format!(
                "{field} must be an even-length hexadecimal string"
            )));
        };
        bytes.push((high << 4) | low);
    }
    if !chunks.remainder().is_empty() {
        return Err(invalid_request(format!(
            "{field} must be an even-length hexadecimal string"
        )));
    }
    Ok(bytes)
}

const fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => byte.checked_sub(b'0'),
        b'a'..=b'f' => match byte.checked_sub(b'a') {
            Some(value) => value.checked_add(10),
            None => None,
        },
        b'A'..=b'F' => match byte.checked_sub(b'A') {
            Some(value) => value.checked_add(10),
            None => None,
        },
        _ => None,
    }
}

fn invalid_request(message: impl Into<String>) -> WireError {
    WireError::new("invalid_request", message)
}
