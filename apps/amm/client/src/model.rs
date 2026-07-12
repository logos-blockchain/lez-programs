use serde::{Deserialize, Serialize};

pub const SCHEMA: &str = "new-position.v1";

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountRead {
    pub id: String,
    pub status: String,
    #[serde(default)]
    pub account: Option<WalletAccount>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct WalletAccount {
    pub program_owner: String,
    pub balance: String,
    pub nonce: String,
    pub data: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigIdRequest {
    pub amm_program_id: String,
}

#[derive(Debug, Deserialize)]
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

#[derive(Debug, Deserialize)]
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairIdsRequest {
    pub amm_program_id: String,
    pub config: AccountRead,
    pub token_a_id: String,
    pub token_b_id: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PositionRequest {
    pub schema: String,
    pub token_a_id: String,
    pub token_b_id: String,
    pub fee_bps: u32,
    #[serde(default)]
    pub max_amount_a_raw: Option<String>,
    #[serde(default)]
    pub max_amount_b_raw: Option<String>,
    #[serde(default)]
    pub slippage_bps: Option<u32>,
    #[serde(default)]
    pub initial_price_real_raw: Option<String>,
    #[serde(default)]
    pub deposit_scale_bps: Option<u32>,
}

#[derive(Clone, Debug, Deserialize)]
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuoteRequest {
    pub network_id: String,
    pub network_fingerprint: String,
    pub amm_program_id: String,
    pub request: PositionRequest,
    pub snapshot: PairSnapshot,
}

#[derive(Debug, Deserialize)]
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

#[derive(Debug, Serialize)]
pub struct Envelope {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Envelope {
    pub fn success(value: serde_json::Value) -> Self {
        Self {
            ok: true,
            value: Some(value),
            error: None,
        }
    }

    pub fn failure(error: impl Into<String>) -> Self {
        Self {
            ok: false,
            value: None,
            error: Some(error.into()),
        }
    }
}
