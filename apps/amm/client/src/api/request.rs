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
pub struct PositionRequest {
    pub schema: String,
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
