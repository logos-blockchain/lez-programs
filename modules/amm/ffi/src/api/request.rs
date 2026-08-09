use serde::Deserialize;

use crate::account::AccountRead;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConfigIdRequest {
    pub amm_program_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TokenIdsRequest {
    pub amm_program_id: String,
    pub config: AccountRead,
    #[serde(default)]
    pub wallet_accounts: Vec<AccountRead>,
    #[serde(default)]
    pub configured_token_ids: Vec<String>,
    #[serde(default)]
    pub recent_token_ids: Vec<String>,
    #[serde(default)]
    pub resolved_token_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ContextRequest {
    pub network_id: String,
    pub network_fingerprint: String,
    pub amm_program_id: String,
    pub wallet_available: bool,
    pub config: AccountRead,
    #[serde(default)]
    pub wallet_accounts: Vec<AccountRead>,
    #[serde(default)]
    pub token_definitions: Vec<AccountRead>,
    #[serde(default)]
    pub configured_token_ids: Vec<String>,
    #[serde(default)]
    pub recent_token_ids: Vec<String>,
    #[serde(default)]
    pub resolved_token_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PairIdsRequest {
    pub amm_program_id: String,
    pub config: AccountRead,
    pub token_a_id: String,
    pub token_b_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SwapPairRequest {
    pub amm_program_id: String,
    pub token_in_id: String,
    pub token_out_id: String,
    pub config: AccountRead,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResolvePoolRequest {
    pub pool: AccountRead,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SwapExactInQuoteRequest {
    pub token_in_id: String,
    pub token_out_id: String,
    pub amount_in_raw: String,
    pub slippage_bps: u32,
    /// Pool account data (hex Borsh `PoolDefinition`). Empty / undecodable ⇒ the
    /// op returns the `no_pool` error.
    pub pool_data: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SwapExactOutQuoteRequest {
    pub token_in_id: String,
    pub token_out_id: String,
    pub amount_out_raw: String,
    pub slippage_bps: u32,
    /// Pool account data (hex Borsh `PoolDefinition`). Empty / undecodable ⇒ the
    /// op returns the `no_pool` error.
    pub pool_data: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PoolIdRequest {
    pub amm_program_id: String,
    pub token_in_id: String,
    pub token_out_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SwapExactInPlanRequest {
    pub amm_program_id: String,
    pub token_in_id: String,
    pub token_out_id: String,
    pub config: AccountRead,
    pub user_input_holding_id: String,
    pub user_output_holding_id: String,
    pub amount_in: String,
    pub min_out: String,
    pub deadline_ms: String,
    /// Pool account data (hex Borsh `PoolDefinition`) — its stored `vault_a_id` /
    /// `vault_b_id` are used verbatim (the guest asserts the vaults in the pool's
    /// creation order, which needn't match the canonical token order).
    pub pool_data: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SwapExactOutPlanRequest {
    pub amm_program_id: String,
    pub token_in_id: String,
    pub token_out_id: String,
    pub config: AccountRead,
    pub user_input_holding_id: String,
    pub user_output_holding_id: String,
    pub amount_out: String,
    pub max_in: String,
    pub deadline_ms: String,
    /// Pool account data (hex Borsh `PoolDefinition`) — its stored `vault_a_id` /
    /// `vault_b_id` are used verbatim (the guest asserts the vaults in the pool's
    /// creation order, which needn't match the canonical token order).
    pub pool_data: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LiquidityQuoteRequest {
    pub token_a_id: String,
    pub token_b_id: String,
    #[serde(default)]
    pub amount_a_raw: Option<String>,
    #[serde(default)]
    pub amount_b_raw: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CreatePoolPlanRequest {
    /// Resolved by the module from `AMM_PROGRAM_BIN` (like every id-deriving op) —
    /// the FFI is stateless and can't read it itself.
    pub amm_program_id: String,
    /// AMM config account read — decoded by `derive_pair` for the `twap_oracle_program_id`
    /// the `current_tick` PDA depends on (same as the swap plan requests).
    pub config: AccountRead,
    pub token_a_id: String,
    pub token_b_id: String,
    #[serde(default)]
    pub amount_a_raw: Option<String>,
    #[serde(default)]
    pub amount_b_raw: Option<String>,
    pub fee_bps: u32,
    pub deadline_ms: String,
    pub user_holding_a_id: String,
    pub user_holding_b_id: String,
    pub user_holding_lp_id: String,
}

/// Prices an `AddLiquidity` into an existing pool — the add counterpart of
/// `LiquidityQuoteRequest`. The two max amounts are the caller's caps (display order);
/// `pool_data` is the hex Borsh `PoolDefinition` (empty ⇒ no pool), same as the swap quotes.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AddLiquidityQuoteRequest {
    pub token_a_id: String,
    pub token_b_id: String,
    pub max_amount_a_raw: String,
    pub max_amount_b_raw: String,
    pub pool_data: String,
}

/// Builds the `AddLiquidity` submission — the add counterpart of `CreatePoolPlanRequest`.
/// `min_lp_raw` is the caller's slippage floor on the LP minted (the guest's
/// `min_amount_liquidity`, applied at submit like the swap plans' `min_out`); `pool_data`
/// supplies the stored vault / LP-definition ids the guest asserts against.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AddLiquidityPlanRequest {
    /// Resolved by the module from `AMM_PROGRAM_BIN` (like every id-deriving op).
    pub amm_program_id: String,
    /// AMM config account read — decoded by `derive_pair` for the `twap_oracle_program_id`
    /// the `current_tick` PDA depends on (same as the swap / create plan requests).
    pub config: AccountRead,
    pub token_a_id: String,
    pub token_b_id: String,
    pub max_amount_a_raw: String,
    pub max_amount_b_raw: String,
    pub min_lp_raw: String,
    pub deadline_ms: String,
    pub user_holding_a_id: String,
    pub user_holding_b_id: String,
    pub user_holding_lp_id: String,
    pub pool_data: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TokenHoldingsRequest {
    pub amm_program_id: String,
    /// AMM config account read — decoded for the `token_program_id` that identifies
    /// which wallet accounts are token holdings.
    pub config: AccountRead,
    #[serde(default)]
    pub wallet_accounts: Vec<AccountRead>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProgramIdRequest {
    pub elf: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PositionRequest {
    pub token_a_id: String,
    pub token_b_id: String,
    pub fee_bps: u32,
    #[serde(default)]
    pub amount_a_raw: Option<String>,
    #[serde(default)]
    pub amount_b_raw: Option<String>,
    #[serde(default)]
    pub max_amount_a_raw: Option<String>,
    #[serde(default)]
    pub max_amount_b_raw: Option<String>,
    #[serde(default)]
    pub slippage_bps: Option<u32>,
    #[serde(default)]
    pub initial_price_real_raw: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PairSnapshot {
    pub config: AccountRead,
    pub token_a: AccountRead,
    pub token_b: AccountRead,
    pub pool: AccountRead,
    pub vault_a: AccountRead,
    pub vault_b: AccountRead,
    pub lp_definition: AccountRead,
    pub lp_lock_holding: AccountRead,
    pub current_tick: AccountRead,
    pub clock: AccountRead,
    pub wallet_available: bool,
    #[serde(default)]
    pub wallet_accounts: Vec<AccountRead>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QuoteRequest {
    pub network_id: String,
    pub network_fingerprint: String,
    pub amm_program_id: String,
    pub request: PositionRequest,
    pub snapshot: PairSnapshot,
}
