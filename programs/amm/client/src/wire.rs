//! Lossless JSON transport adapters for the typed AMM client API.

use std::{error::Error, fmt, str::FromStr};

use amm_core::{
    AmmConfig, Instruction, PoolDefinition, FEE_BPS_DENOMINATOR, MINIMUM_LIQUIDITY,
    SUPPORTED_FEE_TIERS,
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
    discovery::{self, CanonicalPair, PairReadManifest},
    human_price_ratio_to_q64_64, plan_add_liquidity, plan_create_oracle_price_account,
    plan_create_pool, plan_create_price_observations, plan_initialize, plan_remove_liquidity,
    plan_swap_exact_input, plan_swap_exact_output, plan_sync_reserves, plan_update_config,
    quote::{
        self as client_quote, AccountSnapshot, ValidatedFungibleDefinition,
        ValidatedFungibleHolding, ValidatedPoolSnapshot,
    },
    AddLiquidityPlanInput, AmmContext, ClientError, CreateOraclePriceAccountPlanInput,
    CreatePoolPlanInput, CreatePriceObservationsPlanInput, InitializePlanInput, IntentError,
    OpeningLiquidityIntent, PoolContext, PreparedAddLiquidity, PreparedCallerOpeningPair,
    PreparedCreatePool, PreparedOpeningPair, PreparedRemoveLiquidity, PreparedSwapExactInput,
    PreparedSwapExactOutput, PreparedTransaction, RemoveLiquidityPlanInput, SlippageTolerance,
    SwapExactInputPlanInput, SwapExactOutputPlanInput, SyncReservesPlanInput, TransactionError,
    TransactionOperation, TransactionPlan, UpdateConfigPlanInput, WalletPrerequisites,
    SLIPPAGE_BPS_DENOMINATOR,
};

/// Version of the reusable AMM client JSON contract.
///
/// This identifies client payload shape only. It is intentionally unrelated to a deployed AMM
/// ProgramId, ImageID, or release version.
pub const WIRE_SCHEMA: &str = "amm-client.v1";

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

impl From<IntentError> for WireError {
    fn from(error: IntentError) -> Self {
        Self::new(error.code(), error.to_string())
    }
}

impl From<TransactionError> for WireError {
    fn from(error: TransactionError) -> Self {
        Self::new(error.code(), error.to_string())
    }
}

#[derive(Clone, Copy, Deserialize)]
#[serde(transparent)]
struct ProgramIdInput(ProgramId);

impl From<ProgramIdInput> for ProgramId {
    fn from(value: ProgramIdInput) -> Self {
        value.0
    }
}

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
enum PlanRequest {
    Initialize {
        #[serde(rename = "ammProgramId")]
        amm_program_id: ProgramIdInput,
        #[serde(rename = "tokenProgramId")]
        token_program_id: ProgramIdInput,
        #[serde(rename = "twapOracleProgramId")]
        twap_oracle_program_id: ProgramIdInput,
        authority: String,
    },
    UpdateConfig {
        context: ContextInput,
        #[serde(rename = "tokenProgramId")]
        token_program_id: Option<ProgramIdInput>,
        #[serde(rename = "twapOracleProgramId")]
        twap_oracle_program_id: Option<ProgramIdInput>,
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
    PrepareCreatePoolTransaction {
        #[serde(rename = "ammProgramId")]
        amm_program_id: ProgramIdInput,
        config: AccountSnapshotInput,
        snapshots: Box<PairReadSnapshotsInput>,
        #[serde(rename = "firstTokenDefinitionId")]
        first_token_definition_id: String,
        #[serde(rename = "secondTokenDefinitionId")]
        second_token_definition_id: String,
        #[serde(rename = "firstTokenHolding")]
        first_token_holding: AccountSnapshotInput,
        #[serde(rename = "secondTokenHolding")]
        second_token_holding: AccountSnapshotInput,
        #[serde(rename = "liquidityHolding")]
        liquidity_holding: AccountSnapshotInput,
        #[serde(rename = "firstAmount")]
        first_amount: String,
        #[serde(rename = "secondAmount")]
        second_amount: String,
        #[serde(rename = "feeBps")]
        fee_bps: String,
        deadline: String,
    },
    PrepareAddLiquidityTransaction {
        #[serde(rename = "ammProgramId")]
        amm_program_id: ProgramIdInput,
        config: AccountSnapshotInput,
        snapshots: Box<PairReadSnapshotsInput>,
        #[serde(rename = "firstTokenDefinitionId")]
        first_token_definition_id: String,
        #[serde(rename = "secondTokenDefinitionId")]
        second_token_definition_id: String,
        #[serde(rename = "firstTokenHolding")]
        first_token_holding: AccountSnapshotInput,
        #[serde(rename = "secondTokenHolding")]
        second_token_holding: AccountSnapshotInput,
        #[serde(rename = "liquidityHolding")]
        liquidity_holding: AccountSnapshotInput,
        #[serde(rename = "maxFirstAmount")]
        max_first_amount: String,
        #[serde(rename = "maxSecondAmount")]
        max_second_amount: String,
        #[serde(rename = "slippageBps")]
        slippage_bps: String,
        #[serde(rename = "expectedFeeBps")]
        expected_fee_bps: Option<String>,
        deadline: String,
    },
    PrepareRemoveLiquidityTransaction {
        #[serde(rename = "ammProgramId")]
        amm_program_id: ProgramIdInput,
        config: AccountSnapshotInput,
        snapshots: Box<PairReadSnapshotsInput>,
        #[serde(rename = "firstTokenDefinitionId")]
        first_token_definition_id: String,
        #[serde(rename = "secondTokenDefinitionId")]
        second_token_definition_id: String,
        #[serde(rename = "firstTokenHolding")]
        first_token_holding: AccountSnapshotInput,
        #[serde(rename = "secondTokenHolding")]
        second_token_holding: AccountSnapshotInput,
        #[serde(rename = "liquidityHolding")]
        liquidity_holding: AccountSnapshotInput,
        #[serde(rename = "removeLiquidityAmount")]
        remove_liquidity_amount: String,
        #[serde(rename = "slippageBps")]
        slippage_bps: String,
        #[serde(rename = "expectedFeeBps")]
        expected_fee_bps: Option<String>,
        deadline: String,
    },
    PrepareSwapExactInputTransaction {
        #[serde(rename = "ammProgramId")]
        amm_program_id: ProgramIdInput,
        config: AccountSnapshotInput,
        snapshots: Box<PairReadSnapshotsInput>,
        #[serde(rename = "inputTokenDefinitionId")]
        input_token_definition_id: String,
        #[serde(rename = "outputTokenDefinitionId")]
        output_token_definition_id: String,
        #[serde(rename = "inputHolding")]
        input_holding: AccountSnapshotInput,
        #[serde(rename = "outputHolding")]
        output_holding: AccountSnapshotInput,
        #[serde(rename = "amountIn")]
        amount_in: String,
        #[serde(rename = "slippageBps")]
        slippage_bps: String,
        #[serde(rename = "expectedFeeBps")]
        expected_fee_bps: Option<String>,
        deadline: String,
    },
    PrepareSwapExactOutputTransaction {
        #[serde(rename = "ammProgramId")]
        amm_program_id: ProgramIdInput,
        config: AccountSnapshotInput,
        snapshots: Box<PairReadSnapshotsInput>,
        #[serde(rename = "inputTokenDefinitionId")]
        input_token_definition_id: String,
        #[serde(rename = "outputTokenDefinitionId")]
        output_token_definition_id: String,
        #[serde(rename = "inputHolding")]
        input_holding: AccountSnapshotInput,
        #[serde(rename = "outputHolding")]
        output_holding: AccountSnapshotInput,
        #[serde(rename = "exactAmountOut")]
        exact_amount_out: String,
        #[serde(rename = "slippageBps")]
        slippage_bps: String,
        #[serde(rename = "expectedFeeBps")]
        expected_fee_bps: Option<String>,
        deadline: String,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContextInput {
    amm_program_id: ProgramIdInput,
    token_program_id: ProgramIdInput,
    twap_oracle_program_id: ProgramIdInput,
    authority: String,
}

impl ContextInput {
    fn into_context(self) -> Result<AmmContext, WireError> {
        Ok(AmmContext::new(
            self.amm_program_id.into(),
            AmmConfig {
                token_program_id: self.token_program_id.into(),
                twap_oracle_program_id: self.twap_oracle_program_id.into(),
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
    HumanPriceRatioToQ64_64 {
        #[serde(rename = "firstTokenDefinitionId")]
        first_token_definition_id: String,
        #[serde(rename = "secondTokenDefinitionId")]
        second_token_definition_id: String,
        #[serde(rename = "firstAmount")]
        first_amount: String,
        #[serde(rename = "secondAmount")]
        second_amount: String,
        #[serde(rename = "firstTokenDecimals")]
        first_token_decimals: String,
        #[serde(rename = "secondTokenDecimals")]
        second_token_decimals: String,
    },
    DeriveConfigId {
        #[serde(rename = "ammProgramId")]
        amm_program_id: ProgramIdInput,
    },
    InspectConfig {
        #[serde(rename = "ammProgramId")]
        amm_program_id: ProgramIdInput,
        config: AccountSnapshotInput,
    },
    CanonicalPair {
        #[serde(rename = "firstTokenDefinitionId")]
        first_token_definition_id: String,
        #[serde(rename = "secondTokenDefinitionId")]
        second_token_definition_id: String,
    },
    DerivePairReadManifest {
        #[serde(rename = "ammProgramId")]
        amm_program_id: ProgramIdInput,
        config: AccountSnapshotInput,
        #[serde(rename = "firstTokenDefinitionId")]
        first_token_definition_id: String,
        #[serde(rename = "secondTokenDefinitionId")]
        second_token_definition_id: String,
    },
    InspectPair {
        #[serde(rename = "ammProgramId")]
        amm_program_id: ProgramIdInput,
        config: AccountSnapshotInput,
        #[serde(rename = "firstTokenDefinitionId")]
        first_token_definition_id: String,
        #[serde(rename = "secondTokenDefinitionId")]
        second_token_definition_id: String,
        snapshots: PairReadSnapshotsInput,
    },
    PrepareCallerOpeningPair {
        #[serde(rename = "firstTokenDefinitionId")]
        first_token_definition_id: String,
        #[serde(rename = "secondTokenDefinitionId")]
        second_token_definition_id: String,
        #[serde(rename = "desiredPriceQ64_64")]
        desired_price_q64_64: String,
        #[serde(rename = "feeBps")]
        fee_bps: String,
        intent: OpeningLiquidityIntentInput,
    },
    PrepareMinimumOpeningPair {
        #[serde(rename = "desiredPriceQ64_64")]
        desired_price_q64_64: String,
        #[serde(rename = "feeBps")]
        fee_bps: String,
    },
    PrepareOpeningFromTokenA {
        #[serde(rename = "tokenAAmount")]
        token_a_amount: String,
        #[serde(rename = "desiredPriceQ64_64")]
        desired_price_q64_64: String,
        #[serde(rename = "feeBps")]
        fee_bps: String,
    },
    PrepareOpeningFromTokenB {
        #[serde(rename = "tokenBAmount")]
        token_b_amount: String,
        #[serde(rename = "desiredPriceQ64_64")]
        desired_price_q64_64: String,
        #[serde(rename = "feeBps")]
        fee_bps: String,
    },
    ValidateExplicitOpeningPair {
        #[serde(rename = "tokenAAmount")]
        token_a_amount: String,
        #[serde(rename = "tokenBAmount")]
        token_b_amount: String,
        #[serde(rename = "desiredPriceQ64_64")]
        desired_price_q64_64: String,
        #[serde(rename = "feeBps")]
        fee_bps: String,
    },
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
        amm_program_id: ProgramIdInput,
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
        amm_program_id: ProgramIdInput,
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
    amm_program_id: ProgramIdInput,
    config: AccountSnapshotInput,
    snapshot: PoolSnapshotInput,
}

impl PoolStateInput {
    fn validate(self) -> Result<(AmmContext, ValidatedPoolSnapshot), WireError> {
        let config = self.config.into_snapshot()?;
        let context = AmmContext::from_config_account(self.amm_program_id.into(), &config)?;
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PairReadSnapshotsInput {
    pool: AccountSnapshotInput,
    first_token_definition: AccountSnapshotInput,
    second_token_definition: AccountSnapshotInput,
    first_token_vault: AccountSnapshotInput,
    second_token_vault: AccountSnapshotInput,
    liquidity_definition: AccountSnapshotInput,
    lp_lock_holding: AccountSnapshotInput,
    current_tick: AccountSnapshotInput,
    clock: AccountSnapshotInput,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum OpeningLiquidityIntentInput {
    Minimum,
    FirstAmount {
        amount: String,
    },
    SecondAmount {
        amount: String,
    },
    Explicit {
        #[serde(rename = "firstAmount")]
        first_amount: String,
        #[serde(rename = "secondAmount")]
        second_amount: String,
    },
}

impl OpeningLiquidityIntentInput {
    fn into_intent(self) -> Result<OpeningLiquidityIntent, WireError> {
        Ok(match self {
            Self::Minimum => OpeningLiquidityIntent::Minimum,
            Self::FirstAmount { amount } => {
                OpeningLiquidityIntent::FirstAmount(decimal_u128(&amount, "intent.amount")?)
            }
            Self::SecondAmount { amount } => {
                OpeningLiquidityIntent::SecondAmount(decimal_u128(&amount, "intent.amount")?)
            }
            Self::Explicit {
                first_amount,
                second_amount,
            } => OpeningLiquidityIntent::Explicit {
                first_amount: decimal_u128(&first_amount, "intent.firstAmount")?,
                second_amount: decimal_u128(&second_amount, "intent.secondAmount")?,
            },
        })
    }
}

struct OwnedPairReadSnapshots {
    pool: AccountSnapshot,
    first_token_definition: AccountSnapshot,
    second_token_definition: AccountSnapshot,
    first_token_vault: AccountSnapshot,
    second_token_vault: AccountSnapshot,
    liquidity_definition: AccountSnapshot,
    lp_lock_holding: AccountSnapshot,
    current_tick: AccountSnapshot,
    clock: AccountSnapshot,
}

impl PairReadSnapshotsInput {
    fn into_snapshots(self) -> Result<OwnedPairReadSnapshots, WireError> {
        Ok(OwnedPairReadSnapshots {
            pool: self.pool.into_snapshot()?,
            first_token_definition: self.first_token_definition.into_snapshot()?,
            second_token_definition: self.second_token_definition.into_snapshot()?,
            first_token_vault: self.first_token_vault.into_snapshot()?,
            second_token_vault: self.second_token_vault.into_snapshot()?,
            liquidity_definition: self.liquidity_definition.into_snapshot()?,
            lp_lock_holding: self.lp_lock_holding.into_snapshot()?,
            current_tick: self.current_tick.into_snapshot()?,
            clock: self.clock.into_snapshot()?,
        })
    }
}

impl OwnedPairReadSnapshots {
    const fn as_borrowed(&self) -> discovery::PairReadSnapshots<'_> {
        discovery::PairReadSnapshots {
            pool: &self.pool,
            first_token_definition: &self.first_token_definition,
            second_token_definition: &self.second_token_definition,
            first_token_vault: &self.first_token_vault,
            second_token_vault: &self.second_token_vault,
            liquidity_definition: &self.liquidity_definition,
            lp_lock_holding: &self.lp_lock_holding,
            current_tick: &self.current_tick,
            clock: &self.clock,
        }
    }
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
    program_owner: ProgramIdInput,
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
                program_owner: self.program_owner.into(),
                balance: decimal_u128(&self.balance, "account.balance")?,
                data,
                nonce: Nonce(decimal_u128(&self.nonce, "account.nonce")?),
            },
        ))
    }
}

/// Builds a canonical low-level plan or prepares a snapshot-bound task transaction from JSON.
pub fn plan_json(value: Value) -> Result<Value, WireError> {
    validate_wire_schema(&value)?;
    let request: PlanRequest = serde_json::from_value(value)
        .map_err(|error| invalid_request(format!("invalid plan request: {error}")))?;
    versioned(match request {
        PlanRequest::Initialize {
            amm_program_id,
            token_program_id,
            twap_oracle_program_id,
            authority,
        } => transaction_plan_json(&plan_initialize(InitializePlanInput {
            amm_program_id: amm_program_id.into(),
            token_program_id: token_program_id.into(),
            twap_oracle_program_id: twap_oracle_program_id.into(),
            authority: account_id(&authority, "authority")?,
        })),
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
            transaction_plan_json(&plan_update_config(UpdateConfigPlanInput {
                context: &context,
                token_program_id: token_program_id.map(Into::into),
                twap_oracle_program_id: twap_oracle_program_id.map(Into::into),
                new_authority,
            }))
        }
        PlanRequest::CreatePriceObservations {
            context,
            pool_id,
            window_duration,
        } => {
            let context = context.into_context()?;
            transaction_plan_json(&plan_create_price_observations(
                CreatePriceObservationsPlanInput {
                    context: &context,
                    pool_id: account_id(&pool_id, "poolId")?,
                    window_duration: decimal_u64(&window_duration, "windowDuration")?,
                },
            ))
        }
        PlanRequest::CreateOraclePriceAccount {
            context,
            pool_id,
            window_duration,
        } => {
            let context = context.into_context()?;
            transaction_plan_json(&plan_create_oracle_price_account(
                CreateOraclePriceAccountPlanInput {
                    context: &context,
                    pool_id: account_id(&pool_id, "poolId")?,
                    window_duration: decimal_u64(&window_duration, "windowDuration")?,
                },
            ))
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
            transaction_plan_json(&plan_create_pool(CreatePoolPlanInput {
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
            })?)
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
            transaction_plan_json(&plan_add_liquidity(AddLiquidityPlanInput {
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
            }))
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
            transaction_plan_json(&plan_remove_liquidity(RemoveLiquidityPlanInput {
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
            }))
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
            transaction_plan_json(&plan_swap_exact_input(SwapExactInputPlanInput {
                context: &context,
                pool: PoolContext::new(&context, pool_id, &pool)?,
                user_input_holding: account_id(&user_input_holding, "userInputHolding")?,
                user_output_holding: account_id(&user_output_holding, "userOutputHolding")?,
                swap_amount_in: decimal_u128(&swap_amount_in, "swapAmountIn")?,
                min_amount_out: decimal_u128(&min_amount_out, "minAmountOut")?,
                deadline: decimal_u64(&deadline, "deadline")?,
            }))
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
            transaction_plan_json(&plan_swap_exact_output(SwapExactOutputPlanInput {
                context: &context,
                pool: PoolContext::new(&context, pool_id, &pool)?,
                user_input_holding: account_id(&user_input_holding, "userInputHolding")?,
                user_output_holding: account_id(&user_output_holding, "userOutputHolding")?,
                exact_amount_out: decimal_u128(&exact_amount_out, "exactAmountOut")?,
                max_amount_in: decimal_u128(&max_amount_in, "maxAmountIn")?,
                deadline: decimal_u64(&deadline, "deadline")?,
            }))
        }
        PlanRequest::SyncReserves { context, pool } => {
            let context = context.into_context()?;
            let (pool_id, pool) = pool.into_pool()?;
            transaction_plan_json(&plan_sync_reserves(SyncReservesPlanInput {
                context: &context,
                pool: PoolContext::new(&context, pool_id, &pool)?,
            }))
        }
        PlanRequest::PrepareCreatePoolTransaction {
            amm_program_id,
            config,
            snapshots,
            first_token_definition_id,
            second_token_definition_id,
            first_token_holding,
            second_token_holding,
            liquidity_holding,
            first_amount,
            second_amount,
            fee_bps,
            deadline,
        } => {
            let config = config.into_snapshot()?;
            let snapshots = (*snapshots).into_snapshots()?;
            let first_token_holding = first_token_holding.into_snapshot()?;
            let second_token_holding = second_token_holding.into_snapshot()?;
            let liquidity_holding = liquidity_holding.into_snapshot()?;
            let prepared =
                crate::prepare_create_pool_transaction(crate::CreatePoolTransactionInput {
                    amm_program_id: amm_program_id.into(),
                    config: &config,
                    pair: snapshots.as_borrowed(),
                    first_token_definition_id: account_id(
                        &first_token_definition_id,
                        "firstTokenDefinitionId",
                    )?,
                    second_token_definition_id: account_id(
                        &second_token_definition_id,
                        "secondTokenDefinitionId",
                    )?,
                    first_token_holding: &first_token_holding,
                    second_token_holding: &second_token_holding,
                    liquidity_holding: &liquidity_holding,
                    first_amount: decimal_u128(&first_amount, "firstAmount")?,
                    second_amount: decimal_u128(&second_amount, "secondAmount")?,
                    fee_bps: decimal_u128(&fee_bps, "feeBps")?,
                    deadline: decimal_u64(&deadline, "deadline")?,
                })?;
            prepared_transaction_json(&prepared, create_pool_quote_json(*prepared.quote()))
        }
        PlanRequest::PrepareAddLiquidityTransaction {
            amm_program_id,
            config,
            snapshots,
            first_token_definition_id,
            second_token_definition_id,
            first_token_holding,
            second_token_holding,
            liquidity_holding,
            max_first_amount,
            max_second_amount,
            slippage_bps,
            expected_fee_bps,
            deadline,
        } => {
            let config = config.into_snapshot()?;
            let snapshots = (*snapshots).into_snapshots()?;
            let first_token_holding = first_token_holding.into_snapshot()?;
            let second_token_holding = second_token_holding.into_snapshot()?;
            let liquidity_holding = liquidity_holding.into_snapshot()?;
            let prepared =
                crate::prepare_add_liquidity_transaction(crate::AddLiquidityTransactionInput {
                    amm_program_id: amm_program_id.into(),
                    pool_accounts: crate::PoolAccountSnapshots {
                        config: &config,
                        pair: snapshots.as_borrowed(),
                    },
                    first_token_definition_id: account_id(
                        &first_token_definition_id,
                        "firstTokenDefinitionId",
                    )?,
                    second_token_definition_id: account_id(
                        &second_token_definition_id,
                        "secondTokenDefinitionId",
                    )?,
                    first_token_holding: &first_token_holding,
                    second_token_holding: &second_token_holding,
                    liquidity_holding: &liquidity_holding,
                    max_first_amount: decimal_u128(&max_first_amount, "maxFirstAmount")?,
                    max_second_amount: decimal_u128(&max_second_amount, "maxSecondAmount")?,
                    slippage: slippage_tolerance(&slippage_bps)?,
                    expected_fee_bps: optional_decimal_u128(expected_fee_bps, "expectedFeeBps")?,
                    deadline: decimal_u64(&deadline, "deadline")?,
                })?;
            prepared_transaction_json(&prepared, add_liquidity_quote_json(*prepared.quote()))
        }
        PlanRequest::PrepareRemoveLiquidityTransaction {
            amm_program_id,
            config,
            snapshots,
            first_token_definition_id,
            second_token_definition_id,
            first_token_holding,
            second_token_holding,
            liquidity_holding,
            remove_liquidity_amount,
            slippage_bps,
            expected_fee_bps,
            deadline,
        } => {
            let config = config.into_snapshot()?;
            let snapshots = (*snapshots).into_snapshots()?;
            let first_token_holding = first_token_holding.into_snapshot()?;
            let second_token_holding = second_token_holding.into_snapshot()?;
            let liquidity_holding = liquidity_holding.into_snapshot()?;
            let prepared = crate::prepare_remove_liquidity_transaction(
                crate::RemoveLiquidityTransactionInput {
                    amm_program_id: amm_program_id.into(),
                    pool_accounts: crate::PoolAccountSnapshots {
                        config: &config,
                        pair: snapshots.as_borrowed(),
                    },
                    first_token_definition_id: account_id(
                        &first_token_definition_id,
                        "firstTokenDefinitionId",
                    )?,
                    second_token_definition_id: account_id(
                        &second_token_definition_id,
                        "secondTokenDefinitionId",
                    )?,
                    first_token_holding: &first_token_holding,
                    second_token_holding: &second_token_holding,
                    liquidity_holding: &liquidity_holding,
                    remove_liquidity_amount: decimal_u128(
                        &remove_liquidity_amount,
                        "removeLiquidityAmount",
                    )?,
                    slippage: slippage_tolerance(&slippage_bps)?,
                    expected_fee_bps: optional_decimal_u128(expected_fee_bps, "expectedFeeBps")?,
                    deadline: decimal_u64(&deadline, "deadline")?,
                },
            )?;
            prepared_transaction_json(&prepared, remove_liquidity_quote_json(*prepared.quote()))
        }
        PlanRequest::PrepareSwapExactInputTransaction {
            amm_program_id,
            config,
            snapshots,
            input_token_definition_id,
            output_token_definition_id,
            input_holding,
            output_holding,
            amount_in,
            slippage_bps,
            expected_fee_bps,
            deadline,
        } => {
            let config = config.into_snapshot()?;
            let snapshots = (*snapshots).into_snapshots()?;
            let input_holding = input_holding.into_snapshot()?;
            let output_holding = output_holding.into_snapshot()?;
            let prepared = crate::prepare_swap_exact_input_transaction(
                crate::SwapExactInputTransactionInput {
                    amm_program_id: amm_program_id.into(),
                    pool_accounts: crate::PoolAccountSnapshots {
                        config: &config,
                        pair: snapshots.as_borrowed(),
                    },
                    input_token_definition_id: account_id(
                        &input_token_definition_id,
                        "inputTokenDefinitionId",
                    )?,
                    output_token_definition_id: account_id(
                        &output_token_definition_id,
                        "outputTokenDefinitionId",
                    )?,
                    input_holding: &input_holding,
                    output_holding: &output_holding,
                    amount_in: decimal_u128(&amount_in, "amountIn")?,
                    slippage: slippage_tolerance(&slippage_bps)?,
                    expected_fee_bps: optional_decimal_u128(expected_fee_bps, "expectedFeeBps")?,
                    deadline: decimal_u64(&deadline, "deadline")?,
                },
            )?;
            prepared_transaction_json(&prepared, swap_quote_json(*prepared.quote()))
        }
        PlanRequest::PrepareSwapExactOutputTransaction {
            amm_program_id,
            config,
            snapshots,
            input_token_definition_id,
            output_token_definition_id,
            input_holding,
            output_holding,
            exact_amount_out,
            slippage_bps,
            expected_fee_bps,
            deadline,
        } => {
            let config = config.into_snapshot()?;
            let snapshots = (*snapshots).into_snapshots()?;
            let input_holding = input_holding.into_snapshot()?;
            let output_holding = output_holding.into_snapshot()?;
            let prepared = crate::prepare_swap_exact_output_transaction(
                crate::SwapExactOutputTransactionInput {
                    amm_program_id: amm_program_id.into(),
                    pool_accounts: crate::PoolAccountSnapshots {
                        config: &config,
                        pair: snapshots.as_borrowed(),
                    },
                    input_token_definition_id: account_id(
                        &input_token_definition_id,
                        "inputTokenDefinitionId",
                    )?,
                    output_token_definition_id: account_id(
                        &output_token_definition_id,
                        "outputTokenDefinitionId",
                    )?,
                    input_holding: &input_holding,
                    output_holding: &output_holding,
                    exact_amount_out: decimal_u128(&exact_amount_out, "exactAmountOut")?,
                    slippage: slippage_tolerance(&slippage_bps)?,
                    expected_fee_bps: optional_decimal_u128(expected_fee_bps, "expectedFeeBps")?,
                    deadline: decimal_u64(&deadline, "deadline")?,
                },
            )?;
            prepared_transaction_json(&prepared, swap_quote_json(*prepared.quote()))
        }
    })
}

/// Evaluates one reusable AMM quote, discovery operation, or lossless host adapter from JSON.
pub fn quote_json(value: Value) -> Result<Value, WireError> {
    validate_wire_schema(&value)?;
    let request: QuoteRequest = serde_json::from_value(value)
        .map_err(|error| invalid_request(format!("invalid quote request: {error}")))?;
    versioned(match request {
        QuoteRequest::ProtocolConstants => Ok(json!({
            "minimumLiquidity": MINIMUM_LIQUIDITY.to_string(),
            "feeBpsDenominator": FEE_BPS_DENOMINATOR.to_string(),
            "slippageBpsDenominator": SLIPPAGE_BPS_DENOMINATOR.to_string(),
            "supportedFeeTiers": SUPPORTED_FEE_TIERS
                .iter()
                .map(u128::to_string)
                .collect::<Vec<_>>(),
        })),
        QuoteRequest::HumanPriceRatioToQ64_64 {
            first_token_definition_id,
            second_token_definition_id,
            first_amount,
            second_amount,
            first_token_decimals,
            second_token_decimals,
        } => Ok(json!({
            "priceQ64_64": human_price_ratio_to_q64_64(
                account_id(&first_token_definition_id, "firstTokenDefinitionId")?,
                account_id(&second_token_definition_id, "secondTokenDefinitionId")?,
                &first_amount,
                &second_amount,
                decimal_u8(&first_token_decimals, "firstTokenDecimals")?,
                decimal_u8(&second_token_decimals, "secondTokenDecimals")?,
            )?.to_string(),
        })),
        QuoteRequest::DeriveConfigId { amm_program_id } => Ok(json!({
            "configId": discovery::derive_config_id(amm_program_id.into()).to_string(),
        })),
        QuoteRequest::InspectConfig {
            amm_program_id,
            config,
        } => {
            let config = config.into_snapshot()?;
            let context = discovery::inspect_config(amm_program_id.into(), &config)?;
            Ok(amm_context_json(&context))
        }
        QuoteRequest::CanonicalPair {
            first_token_definition_id,
            second_token_definition_id,
        } => {
            let pair = discovery::canonical_pair(
                account_id(&first_token_definition_id, "firstTokenDefinitionId")?,
                account_id(&second_token_definition_id, "secondTokenDefinitionId")?,
            )?;
            Ok(canonical_pair_json(pair))
        }
        QuoteRequest::DerivePairReadManifest {
            amm_program_id,
            config,
            first_token_definition_id,
            second_token_definition_id,
        } => {
            let config = config.into_snapshot()?;
            let context = discovery::inspect_config(amm_program_id.into(), &config)?;
            let manifest = discovery::derive_pair_read_manifest(
                &context,
                account_id(&first_token_definition_id, "firstTokenDefinitionId")?,
                account_id(&second_token_definition_id, "secondTokenDefinitionId")?,
            )?;
            Ok(pair_read_manifest_json(manifest))
        }
        QuoteRequest::InspectPair {
            amm_program_id,
            config,
            first_token_definition_id,
            second_token_definition_id,
            snapshots,
        } => {
            let config = config.into_snapshot()?;
            let context = discovery::inspect_config(amm_program_id.into(), &config)?;
            let snapshots = snapshots.into_snapshots()?;
            let inspected = discovery::inspect_pair(
                &context,
                account_id(&first_token_definition_id, "firstTokenDefinitionId")?,
                account_id(&second_token_definition_id, "secondTokenDefinitionId")?,
                snapshots.as_borrowed(),
            )?;
            Ok(pair_inspection_json(inspected))
        }
        QuoteRequest::PrepareCallerOpeningPair {
            first_token_definition_id,
            second_token_definition_id,
            desired_price_q64_64,
            fee_bps,
            intent,
        } => Ok(prepared_caller_opening_pair_json(
            crate::prepare_caller_opening_pair(
                account_id(&first_token_definition_id, "firstTokenDefinitionId")?,
                account_id(&second_token_definition_id, "secondTokenDefinitionId")?,
                decimal_u128(&desired_price_q64_64, "desiredPriceQ64_64")?,
                decimal_u128(&fee_bps, "feeBps")?,
                intent.into_intent()?,
            )?,
        )),
        QuoteRequest::PrepareMinimumOpeningPair {
            desired_price_q64_64,
            fee_bps,
        } => Ok(prepared_opening_pair_json(
            crate::prepare_minimum_opening_pair(
                decimal_u128(&desired_price_q64_64, "desiredPriceQ64_64")?,
                decimal_u128(&fee_bps, "feeBps")?,
            )?,
        )),
        QuoteRequest::PrepareOpeningFromTokenA {
            token_a_amount,
            desired_price_q64_64,
            fee_bps,
        } => Ok(prepared_opening_pair_json(
            crate::prepare_opening_from_token_a(
                decimal_u128(&token_a_amount, "tokenAAmount")?,
                decimal_u128(&desired_price_q64_64, "desiredPriceQ64_64")?,
                decimal_u128(&fee_bps, "feeBps")?,
            )?,
        )),
        QuoteRequest::PrepareOpeningFromTokenB {
            token_b_amount,
            desired_price_q64_64,
            fee_bps,
        } => Ok(prepared_opening_pair_json(
            crate::prepare_opening_from_token_b(
                decimal_u128(&token_b_amount, "tokenBAmount")?,
                decimal_u128(&desired_price_q64_64, "desiredPriceQ64_64")?,
                decimal_u128(&fee_bps, "feeBps")?,
            )?,
        )),
        QuoteRequest::ValidateExplicitOpeningPair {
            token_a_amount,
            token_b_amount,
            desired_price_q64_64,
            fee_bps,
        } => Ok(prepared_opening_pair_json(
            crate::validate_explicit_opening_pair(
                decimal_u128(&token_a_amount, "tokenAAmount")?,
                decimal_u128(&token_b_amount, "tokenBAmount")?,
                decimal_u128(&desired_price_q64_64, "desiredPriceQ64_64")?,
                decimal_u128(&fee_bps, "feeBps")?,
            )?,
        )),
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
            let context = AmmContext::from_config_account(amm_program_id.into(), &config)?;
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
            let context = AmmContext::from_config_account(amm_program_id.into(), &config)?;
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
            swap_quote_with_pool_spot_change_json(snapshot.pool(), quote)
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
            prepared_swap_exact_input_json(snapshot.pool(), prepared)
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
            swap_quote_with_pool_spot_change_json(snapshot.pool(), quote)
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
            swap_quote_with_pool_spot_change_json(snapshot.pool(), quote)
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
            prepared_swap_exact_output_json(snapshot.pool(), prepared)
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
            swap_quote_with_pool_spot_change_json(snapshot.pool(), quote)
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
    })
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
        "instructionArgs": instruction_args_json(plan.instruction()),
        "programId": program_id_words(plan.program_id()),
        "accounts": accounts,
        "affectedAccountIds": plan
            .affected_account_ids()
            .into_iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>(),
        "instructionWords": instruction_words,
    }))
}

fn instruction_args_json(instruction: &Instruction) -> Value {
    match instruction {
        Instruction::Initialize {
            token_program_id,
            twap_oracle_program_id,
            authority,
        } => json!({
            "tokenProgramId": program_id_words(*token_program_id),
            "twapOracleProgramId": program_id_words(*twap_oracle_program_id),
            "authority": authority.to_string(),
        }),
        Instruction::UpdateConfig {
            token_program_id,
            twap_oracle_program_id,
            new_authority,
        } => json!({
            "tokenProgramId": token_program_id.map(program_id_words),
            "twapOracleProgramId": twap_oracle_program_id.map(program_id_words),
            "newAuthority": new_authority.map(|authority| authority.to_string()),
        }),
        Instruction::CreatePriceObservations { window_duration }
        | Instruction::CreateOraclePriceAccount { window_duration } => json!({
            "windowDuration": window_duration.to_string(),
        }),
        Instruction::NewDefinition {
            token_a_amount,
            token_b_amount,
            fees,
            deadline,
        } => json!({
            "tokenAAmount": token_a_amount.to_string(),
            "tokenBAmount": token_b_amount.to_string(),
            "fees": fees.to_string(),
            "deadline": deadline.to_string(),
        }),
        Instruction::AddLiquidity {
            min_amount_liquidity,
            max_amount_to_add_token_a,
            max_amount_to_add_token_b,
            deadline,
        } => json!({
            "minAmountLiquidity": min_amount_liquidity.to_string(),
            "maxAmountToAddTokenA": max_amount_to_add_token_a.to_string(),
            "maxAmountToAddTokenB": max_amount_to_add_token_b.to_string(),
            "deadline": deadline.to_string(),
        }),
        Instruction::RemoveLiquidity {
            remove_liquidity_amount,
            min_amount_to_remove_token_a,
            min_amount_to_remove_token_b,
            deadline,
        } => json!({
            "removeLiquidityAmount": remove_liquidity_amount.to_string(),
            "minAmountToRemoveTokenA": min_amount_to_remove_token_a.to_string(),
            "minAmountToRemoveTokenB": min_amount_to_remove_token_b.to_string(),
            "deadline": deadline.to_string(),
        }),
        Instruction::SwapExactInput {
            swap_amount_in,
            min_amount_out,
            deadline,
        } => json!({
            "swapAmountIn": swap_amount_in.to_string(),
            "minAmountOut": min_amount_out.to_string(),
            "deadline": deadline.to_string(),
        }),
        Instruction::SwapExactOutput {
            exact_amount_out,
            max_amount_in,
            deadline,
        } => json!({
            "exactAmountOut": exact_amount_out.to_string(),
            "maxAmountIn": max_amount_in.to_string(),
            "deadline": deadline.to_string(),
        }),
        Instruction::SyncReserves => json!({}),
    }
}

fn validate_wire_schema(value: &Value) -> Result<(), WireError> {
    let Some(schema) = value.get("schema") else {
        return Ok(());
    };
    let Some(schema) = schema.as_str() else {
        return Err(invalid_request("schema must be a string"));
    };
    if schema == WIRE_SCHEMA {
        Ok(())
    } else {
        Err(WireError::new(
            "unsupported_schema",
            format!("unsupported AMM client schema {schema}"),
        ))
    }
}

fn versioned(result: Result<Value, WireError>) -> Result<Value, WireError> {
    let mut value = result?;
    let Some(object) = value.as_object_mut() else {
        return Err(WireError::new(
            "response_serialization_failed",
            "AMM client response must be a JSON object",
        ));
    };
    object.insert(
        String::from("schema"),
        Value::String(String::from(WIRE_SCHEMA)),
    );
    Ok(value)
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

fn amm_context_json(context: &AmmContext) -> Value {
    json!({
        "ammProgramId": program_id_words(context.amm_program_id),
        "configId": context.config_id().to_string(),
        "tokenProgramId": program_id_words(context.token_program_id()),
        "twapOracleProgramId": program_id_words(context.twap_oracle_program_id()),
        "authority": context.config.authority.to_string(),
    })
}

fn canonical_pair_json(pair: CanonicalPair) -> Value {
    json!({
        "tokenAId": pair.token_a_id().to_string(),
        "tokenBId": pair.token_b_id().to_string(),
    })
}

fn pair_read_manifest_json(manifest: PairReadManifest) -> Value {
    let first_token = manifest.first_token();
    let second_token = manifest.second_token();
    json!({
        "canonicalPair": canonical_pair_json(manifest.canonical_pair()),
        "firstToken": {
            "definitionId": first_token.definition_id().to_string(),
            "vaultId": first_token.vault_id().to_string(),
        },
        "secondToken": {
            "definitionId": second_token.definition_id().to_string(),
            "vaultId": second_token.vault_id().to_string(),
        },
        "configId": manifest.config_id().to_string(),
        "poolId": manifest.pool_id().to_string(),
        "liquidityDefinitionId": manifest.liquidity_definition_id().to_string(),
        "lpLockHoldingId": manifest.lp_lock_holding_id().to_string(),
        "currentTickId": manifest.current_tick_id().to_string(),
        "clockId": manifest.clock_id().to_string(),
    })
}

fn pair_inspection_json(inspection: discovery::PairInspection) -> Value {
    match inspection {
        discovery::PairInspection::Missing(missing) => json!({
            "status": "missing",
            "manifest": pair_read_manifest_json(missing.manifest()),
            "firstTokenDefinition": fungible_definition_json(missing.first_token_definition()),
            "secondTokenDefinition": fungible_definition_json(missing.second_token_definition()),
            "firstVault": missing_vault_json(missing.first_vault()),
            "secondVault": missing_vault_json(missing.second_vault()),
            "clock": clock_json(missing.clock()),
        }),
        discovery::PairInspection::Active(active) => {
            let snapshot = active.pool();
            let pool = snapshot.pool();
            json!({
                "status": "active",
                "manifest": pair_read_manifest_json(active.manifest()),
                "callerOrder": match active.caller_order() {
                    PairOrder::Stored => "stored",
                    PairOrder::Reversed => "reversed",
                },
                "stored": {
                    "tokenADefinitionId": pool.definition_token_a_id.to_string(),
                    "tokenBDefinitionId": pool.definition_token_b_id.to_string(),
                    "vaultAId": pool.vault_a_id.to_string(),
                    "vaultBId": pool.vault_b_id.to_string(),
                    "liquidityDefinitionId": pool.liquidity_pool_id.to_string(),
                    "lpLockHoldingId": active.lp_lock_holding().account_id().to_string(),
                    "reserveA": pool.reserve_a.to_string(),
                    "reserveB": pool.reserve_b.to_string(),
                    "vaultABalance": snapshot.vault_a().balance().to_string(),
                    "vaultBBalance": snapshot.vault_b().balance().to_string(),
                    "liquidityPoolSupply": pool.liquidity_pool_supply.to_string(),
                    "lpLockBalance": active.lp_lock_holding().balance().to_string(),
                    "feeBps": pool.fees.to_string(),
                },
                "storedSpotPriceQ64_64": active.stored_spot_price_q64_64().to_string(),
                "currentTick": {
                    "tick": active.current_tick().tick.to_string(),
                    "lastUpdated": active.current_tick().last_updated.to_string(),
                },
                "clock": clock_json(active.clock()),
            })
        }
    }
}

fn fungible_definition_json(definition: &ValidatedFungibleDefinition) -> Value {
    json!({
        "id": definition.account_id().to_string(),
        "totalSupply": definition.total_supply().to_string(),
        "authority": definition.authority().map(|authority| authority.to_string()),
    })
}

fn missing_vault_json(vault: discovery::MissingVaultState) -> Value {
    match vault {
        discovery::MissingVaultState::Uninitialized => json!({
            "status": "uninitialized",
        }),
        discovery::MissingVaultState::ExistingFungible { balance } => json!({
            "status": "existing_fungible",
            "balance": balance.to_string(),
        }),
    }
}

fn clock_json(clock: discovery::ValidatedClockSnapshot) -> Value {
    json!({
        "blockId": clock.block_id().to_string(),
        "timestamp": clock.timestamp().to_string(),
    })
}

fn prepared_opening_pair_json(prepared: PreparedOpeningPair) -> Value {
    json!({
        "desiredPriceQ64_64": prepared.desired_price_q64_64.to_string(),
        "actualPriceQ64_64": prepared.actual_price_q64_64.to_string(),
        "tokenAAmount": prepared.token_a_amount.to_string(),
        "tokenBAmount": prepared.token_b_amount.to_string(),
        "feeBps": prepared.fee_bps.to_string(),
        "quote": create_pool_quote_json(prepared.quote),
    })
}

fn prepared_caller_opening_pair_json(prepared: PreparedCallerOpeningPair) -> Value {
    json!({
        "callerOrder": match prepared.caller_order() {
            PairOrder::Stored => "stored",
            PairOrder::Reversed => "reversed",
        },
        "firstAmount": prepared.first_amount().to_string(),
        "secondAmount": prepared.second_amount().to_string(),
        "stored": prepared_opening_pair_json(*prepared.stored()),
    })
}

fn prepared_transaction_json<Q>(
    prepared: &PreparedTransaction<Q>,
    quote: Value,
) -> Result<Value, WireError> {
    Ok(json!({
        "operation": transaction_operation_name(prepared.operation()),
        "quote": quote,
        "callerAmounts": {
            "first": prepared.caller_amounts().first().to_string(),
            "second": prepared.caller_amounts().second().to_string(),
        },
        "plan": transaction_plan_json(prepared.plan())?,
        "quoteCommitment": quote_commitment_hex(prepared.quote_commitment()),
        "affectedAccountIds": prepared
            .affected_account_ids()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        "walletPrerequisites": wallet_prerequisites_json(prepared.wallet_prerequisites()),
        "deadline": prepared.deadline().to_string(),
        "poolSpotChangeBps": prepared.pool_spot_change_bps().map(|bps| bps.to_string()),
    }))
}

const fn transaction_operation_name(operation: TransactionOperation) -> &'static str {
    match operation {
        TransactionOperation::CreatePool => "create_pool",
        TransactionOperation::AddLiquidity => "add_liquidity",
        TransactionOperation::RemoveLiquidity => "remove_liquidity",
        TransactionOperation::SwapExactInput => "swap_exact_input",
        TransactionOperation::SwapExactOutput => "swap_exact_output",
    }
}

fn quote_commitment_hex(commitment: crate::QuoteCommitment) -> String {
    commitment
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn wallet_prerequisites_json(prerequisites: &WalletPrerequisites) -> Value {
    json!({
        "signerAccountIds": prerequisites
            .signer_account_ids()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        "freshAccountIds": prerequisites
            .fresh_account_ids()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        "funding": prerequisites
            .funding()
            .iter()
            .map(|requirement| json!({
                "holdingAccountId": requirement.holding_account_id().to_string(),
                "tokenDefinitionId": requirement.token_definition_id().to_string(),
                "available": requirement.available().to_string(),
                "required": requirement.required().to_string(),
            }))
            .collect::<Vec<_>>(),
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

fn swap_quote_with_pool_spot_change_json(
    before: &PoolDefinition,
    quote: SwapQuote,
) -> Result<Value, WireError> {
    let pool_spot_change_bps = crate::pool_spot_change_bps(before, &quote)?;
    let mut value = swap_quote_json(quote);
    let Some(object) = value.as_object_mut() else {
        return Err(WireError::new(
            "response_serialization_failed",
            "swap quote response must be a JSON object",
        ));
    };
    object.insert(
        String::from("poolSpotChangeBps"),
        Value::String(pool_spot_change_bps.to_string()),
    );
    Ok(value)
}

fn prepared_swap_exact_input_json(
    before: &PoolDefinition,
    prepared: PreparedSwapExactInput,
) -> Result<Value, WireError> {
    Ok(json!({
        "quote": swap_quote_with_pool_spot_change_json(before, prepared.quote)?,
        "instructionArgs": {
            "swapAmountIn": prepared.swap_amount_in.to_string(),
            "minAmountOut": prepared.min_amount_out.to_string(),
        },
    }))
}

fn prepared_swap_exact_output_json(
    before: &PoolDefinition,
    prepared: PreparedSwapExactOutput,
) -> Result<Value, WireError> {
    Ok(json!({
        "quote": swap_quote_with_pool_spot_change_json(before, prepared.quote)?,
        "instructionArgs": {
            "exactAmountOut": prepared.exact_amount_out.to_string(),
            "maxAmountIn": prepared.max_amount_in.to_string(),
        },
    }))
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

const fn program_id_words(program_id: ProgramId) -> ProgramId {
    program_id
}

fn account_id(value: &str, field: &str) -> Result<AccountId, WireError> {
    let account_id = AccountId::from_str(value)
        .map_err(|error| invalid_request(format!("{field} is not a valid account ID: {error}")))?;
    if account_id.to_string() != value {
        return Err(invalid_request(format!(
            "{field} must use canonical base58 encoding"
        )));
    }
    Ok(account_id)
}

fn decimal_u128(value: &str, field: &str) -> Result<u128, WireError> {
    decimal(value, field)
}

fn decimal_u64(value: &str, field: &str) -> Result<u64, WireError> {
    decimal(value, field)
}

fn decimal_u8(value: &str, field: &str) -> Result<u8, WireError> {
    decimal(value, field)
}

fn optional_decimal_u128(value: Option<String>, field: &str) -> Result<Option<u128>, WireError> {
    value
        .as_deref()
        .map(|value| decimal_u128(value, field))
        .transpose()
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
