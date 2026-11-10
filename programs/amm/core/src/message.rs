use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::account::AccountId;

use crate::AmmConfig;

/// Public actor messages. NewDefinition goes to the config actor; other pool operations
/// go to the pool actor. Initialization and admin
/// operations go to the authorized owner's/authority's AMM actor, which forwards them.
/// Token destinations must be declared so credits settle in this transaction. All holdings
/// must exist, except the initial LP holding (created with the pool).
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Message {
    /// Sent to the authorized namespace owner's actor. Creates the config PDA derived
    /// from that owner and `nonce`; deployment ids and fees are immutable afterward.
    Initialize {
        nonce: [u8; 32],
        config: AmmConfig,
    },
    /// Sent to the authorized current admin's actor, which forwards to `config`.
    UpdateConfig {
        config: AccountId,
        new_authority: AccountId,
    },
    /// Sent to `config`, which admits the derived pool with its PDA seed. Both deposit
    /// holdings and the fresh LP holding authorize; the permanent minimum LP lock is
    /// deducted from the initial supply before the transaction finishes.
    NewDefinition {
        config: AccountId,
        definition_token_a_id: AccountId,
        definition_token_b_id: AccountId,
        user_holding_a: AccountId,
        user_holding_b: AccountId,
        user_holding_lp: AccountId,
        token_a_amount: u128,
        token_b_amount: u128,
        /// Last valid block timestamp in milliseconds (inclusive).
        deadline: u64,
    },
    /// Both deposit holdings authorize. The existing LP holding receives minted LPs.
    AddLiquidity {
        user_holding_a: AccountId,
        user_holding_b: AccountId,
        user_holding_lp: AccountId,
        min_amount_liquidity: u128,
        max_amount_to_add_token_a: u128,
        max_amount_to_add_token_b: u128,
        deadline: u64,
    },
    /// The LP holding authorizes its burn; the two output holdings only receive.
    RemoveLiquidity {
        user_holding_a: AccountId,
        user_holding_b: AccountId,
        user_holding_lp: AccountId,
        remove_liquidity_amount: u128,
        min_amount_to_remove_token_a: u128,
        min_amount_to_remove_token_b: u128,
        deadline: u64,
    },
    /// The input holding authorizes. Its token definition selects the direction.
    SwapExactInput {
        user_input_holding: AccountId,
        user_output_holding: AccountId,
        swap_amount_in: u128,
        min_amount_out: u128,
        deadline: u64,
    },
    /// Like exact input, with a fixed output and a maximum debit instead.
    SwapExactOutput {
        user_input_holding: AccountId,
        user_output_holding: AccountId,
        exact_amount_out: u128,
        max_amount_in: u128,
        deadline: u64,
    },
    /// Permissionless: incorporate direct vault donations into reserves and notify the oracle.
    SyncReserves,
    CreatePriceObservations {
        window_duration: u64,
    },
    CreateOraclePriceAccount {
        window_duration: u64,
    },
    /// Sent to the authorized current admin's actor. The source is derived from
    /// `(config, token)`; `destination` must already hold that token.
    WithdrawProtocolFees {
        config: AccountId,
        token: AccountId,
        destination: AccountId,
        amount: u128,
    },
    /// Reply to the sender with a `lee_core::program::StateReply`. Busy actors reject reads.
    ReadState,
}

/// Decoded only for messages from this AMM deployment. No generic forwarding or arbitrary
/// seed grants: each privileged message is constructed by an authorized public handler.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Internal {
    InitializeConfig {
        nonce: [u8; 32],
        config: AmmConfig,
    },
    UpdateConfig {
        new_authority: AccountId,
    },
    WithdrawProtocolFees {
        token: AccountId,
        destination: AccountId,
        amount: u128,
    },
    ReadState,
    Complete,
    NewPool {
        request: Box<Message>,
    },
}
