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
pub struct SwapPlanRequest {
    pub amm_program_id: String,
    pub token_in_id: String,
    pub token_out_id: String,
    pub config: AccountRead,
    pub user_input_holding_id: String,
    pub user_output_holding_id: String,
    pub amount_in: String,
    pub min_out: String,
    pub deadline_ms: String,
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlanRequest {
    pub network_id: String,
    pub network_fingerprint: String,
    pub amm_program_id: String,
    pub request: PositionRequest,
    pub snapshot: PairSnapshot,
    pub quote_hash: String,
    pub now_ms: u64,
    #[serde(default)]
    pub fresh_lp: Option<AccountRead>,
}
