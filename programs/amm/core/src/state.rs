use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::account::AccountId;
use serde::{Deserialize, Serialize};
use token_core::TokenHolding;

use crate::Message;

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum AmmState {
    Config(AmmConfig),
    Pool(PoolDefinition),
    /// A synchronous read conversation. Never left pending by a successful transaction.
    Pending(Box<Pending>),
    /// External calls are running. Only the actor's own completion message unlocks it.
    Settling(Box<AmmState>),
}

impl AmmState {
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.is_empty() {
            None
        } else {
            Some(borsh::from_slice(bytes).expect("AMM actor state must decode"))
        }
    }

    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        borsh::to_vec(self).expect("borsh serialization is infallible")
    }
}

/// An immutable deployment and fee configuration, with a transferable admin authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct AmmConfig {
    /// Deployed token program account address, shared by every pool in this namespace.
    pub token_program_id: AccountId,
    /// Oracle deployment implementing the AMM-facing message contract.
    pub twap_oracle_program_id: AccountId,
    pub authority: AccountId,
    /// Swap fee in basis points, strictly below 10,000.
    pub swap_fee_bps: u128,
    /// Protocol's fraction of the swap fee in basis points, at most 10,000.
    pub protocol_fee_bps: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct PoolDefinition {
    /// Namespace whose authenticated config supplies the token/oracle programs and fees.
    pub config_id: AccountId,
    pub definition_token_a_id: AccountId,
    pub definition_token_b_id: AccountId,
    pub vault_a_id: AccountId,
    pub vault_b_id: AccountId,
    pub liquidity_pool_id: AccountId,
    /// Includes the permanently locked minimum. Direct LP burns donate to remaining LPs.
    pub liquidity_pool_supply: u128,
    pub reserve_a: u128,
    pub reserve_b: u128,
}

/// Continuation data written only by AMM code. Replies must match both the expected account
/// and program; transaction authors cannot supply token or configuration snapshots.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Pending {
    pub config_id: AccountId,
    pub config: Option<AmmConfig>,
    pub pool: Option<PoolDefinition>,
    pub request: Message,
    /// Stored explicitly so the IDL describes the full state without external Rust types.
    pub expected_account: AccountId,
    pub expected_program: AccountId,
    pub remaining: Vec<AccountId>,
    pub holdings: Vec<Option<TokenHolding>>,
}
