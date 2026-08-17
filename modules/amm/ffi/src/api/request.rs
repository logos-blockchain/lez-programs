use serde::Deserialize;

use crate::account::AccountRead;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConfigIdRequest {
    pub amm_program_id: String,
}

/// Decodes the singleton AMM config account. `config` is the read of the config PDA the module
/// derives from `amm_program_id`; the op returns the authority + program ids (or
/// `{ status:"error", error:"config_unavailable" }` when the config isn't on-chain yet).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConfigAccountRequest {
    pub amm_program_id: String,
    pub config: AccountRead,
}

/// Builds the `UpdateConfig` submission transferring admin authority. `config` is the read of the
/// config PDA (the current admin — the sole signer — is decoded from it); `new_authority_id` is
/// hex (the module normalizes base58→hex).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TransferOwnershipPlanRequest {
    pub amm_program_id: String,
    pub config: AccountRead,
    pub new_authority_id: String,
}

/// Builds the `CreatePriceObservations` submission — seeds the pool's TWAP observations feed for a
/// window. `config` is the read of the config PDA (its `twap_oracle_program_id` seeds the feed
/// PDAs); `token_ids` are hex; `window_duration_ms` is the TWAP window in milliseconds.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CreatePriceObservationsPlanRequest {
    pub amm_program_id: String,
    pub config: AccountRead,
    pub token_a_id: String,
    pub token_b_id: String,
    pub window_duration_ms: u64,
}

/// Builds the `CreateOraclePriceAccount` submission — creates the pool's TWAP oracle price account
/// for a window. Same inputs as `CreatePriceObservationsPlanRequest`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CreateOraclePriceAccountPlanRequest {
    pub amm_program_id: String,
    pub config: AccountRead,
    pub token_a_id: String,
    pub token_b_id: String,
    pub window_duration_ms: u64,
}

/// Resolves an app-provided set of token ids into selector rows. `token_ids` are hex — the
/// module normalizes base58→hex and reads each definition into `token_definitions` (keyed by
/// hex id) plus the wallet accounts; the FFI is stateless and reads nothing itself.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResolveTokensRequest {
    pub amm_program_id: String,
    pub config: AccountRead,
    #[serde(default)]
    pub token_ids: Vec<String>,
    #[serde(default)]
    pub wallet_accounts: Vec<AccountRead>,
    #[serde(default)]
    pub token_definitions: Vec<AccountRead>,
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
    pub amount_in: String,
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
    pub amount_out: String,
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
pub struct CreatePoolQuoteRequest {
    pub token_a_id: String,
    pub token_b_id: String,
    /// The opening price as a `Q64.64` fixed-point value (token B per token A, canonical
    /// order). Required only in the price-only mode (no `amount_*_raw`), where it drives the
    /// minimum opening deposit; when amounts are supplied the op derives the price from them.
    #[serde(default)]
    pub price: Option<String>,
    #[serde(default)]
    pub amount_a: Option<String>,
    #[serde(default)]
    pub amount_b: Option<String>,
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
    pub amount_a: Option<String>,
    #[serde(default)]
    pub amount_b: Option<String>,
    pub fee_bps: u32,
    pub deadline_ms: String,
    pub user_holding_a_id: String,
    pub user_holding_b_id: String,
    pub user_holding_lp_id: String,
}

/// Prices an `AddLiquidity` into an existing pool — the add counterpart of
/// `CreatePoolQuoteRequest`. The two max amounts are the caller's caps (display order);
/// `pool_data` is the hex Borsh `PoolDefinition` (empty ⇒ no pool), same as the swap quotes.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AddLiquidityQuoteRequest {
    pub token_a_id: String,
    pub token_b_id: String,
    pub max_amount_a: String,
    pub max_amount_b: String,
    /// Slippage tolerance in basis points — the quote returns `minimumLp`, the LP floor
    /// the submit accepts (like the swap quotes take `slippageBps` → `minReceived`).
    #[serde(default)]
    pub slippage_bps: u32,
    pub pool_data: String,
}

/// Builds the `AddLiquidity` submission — the add counterpart of `CreatePoolPlanRequest`.
/// `min_lp` is the caller's slippage floor on the LP minted (the guest's
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
    pub max_amount_a: String,
    pub max_amount_b: String,
    pub min_lp: String,
    pub deadline_ms: String,
    pub user_holding_a_id: String,
    pub user_holding_b_id: String,
    pub user_holding_lp_id: String,
    pub pool_data: String,
}

/// Prices burning `lp_amount` of an existing pool's LP. `slippage_bps` sets the
/// `minimumAmount*Raw` floors the submit enforces (the guest requires both nonzero and
/// `withdraw >= min`). `pool_data` is the hex Borsh `PoolDefinition` (empty ⇒ no pool).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RemoveLiquidityQuoteRequest {
    pub token_a_id: String,
    pub token_b_id: String,
    pub lp_amount: String,
    #[serde(default)]
    pub slippage_bps: u32,
    pub pool_data: String,
}

/// Builds the `RemoveLiquidity` submission — the remove counterpart of
/// `AddLiquidityPlanRequest`. `min_amount_*_raw` are the caller's slippage floors on the
/// tokens withdrawn (the guest's `min_amount_to_remove_token_*`, both applied at submit and
/// required nonzero); `pool_data` supplies the stored vault / LP-definition ids the guest
/// asserts against. Unlike add/create there is no fresh holding — the caller's existing LP
/// holding is burned and the existing token a/b holdings receive the withdrawal.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RemoveLiquidityPlanRequest {
    /// Resolved by the module from `AMM_PROGRAM_BIN` (like every id-deriving op).
    pub amm_program_id: String,
    /// AMM config account read — decoded by `derive_pair` for the `twap_oracle_program_id`
    /// the `current_tick` PDA depends on (same as the swap / add plan requests).
    pub config: AccountRead,
    pub token_a_id: String,
    pub token_b_id: String,
    pub lp_amount: String,
    pub min_amount_a: String,
    pub min_amount_b: String,
    pub deadline_ms: String,
    pub user_holding_a_id: String,
    pub user_holding_b_id: String,
    pub user_holding_lp_id: String,
    pub pool_data: String,
}

/// Builds the `SyncReserves` submission — a permissionless keeper op refreshing the pool's
/// stored reserves to the live vault balances (and its TWAP tick). No amounts / deadline /
/// holdings: it is a unit instruction over pool-derived accounts. `pool_data` supplies the
/// stored vault ids the guest asserts against.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SyncReservesPlanRequest {
    /// Resolved by the module from `AMM_PROGRAM_BIN` (like every id-deriving op).
    pub amm_program_id: String,
    /// AMM config account read — decoded by `derive_pair` for the `twap_oracle_program_id`
    /// the `current_tick` PDA depends on (same as the swap / liquidity plan requests).
    pub config: AccountRead,
    pub token_a_id: String,
    pub token_b_id: String,
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

/// No inputs — `fee_tiers` enumerates `amm_core::SUPPORTED_FEE_TIERS`. An empty struct so the
/// op keeps the uniform `call::<T>` request-decoding path (the module sends `{}`).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FeeTiersRequest {}
