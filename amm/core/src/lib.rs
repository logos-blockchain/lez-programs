//! This crate contains core data structures and utilities for the AMM Program.

use borsh::{BorshDeserialize, BorshSerialize};
use nssa_core::{
    account::{AccountId, Data},
    program::{PdaSeed, ProgramId},
};
use serde::{Deserialize, Serialize};

// These stable seed bytes are part of the PDA derivation scheme and must stay unchanged for
// compatibility.
const LIQUIDITY_TOKEN_PDA_SEED: [u8; 32] = [0; 32];
const LP_LOCK_HOLDING_PDA_SEED: [u8; 32] = [1; 32];

/// AMM Program Instruction.
#[derive(Serialize, Deserialize)]
pub enum Instruction {
    /// Initializes a new Pool (or re-initializes an inactive Pool).
    ///
    /// On initialization, `MINIMUM_LIQUIDITY` LP tokens are permanently locked
    /// in the LP-lock holding PDA; the caller receives `initial_lp - MINIMUM_LIQUIDITY`.
    ///
    /// Required accounts:
    /// - AMM Pool
    /// - Vault Holding Account for Token A
    /// - Vault Holding Account for Token B
    /// - Pool Liquidity Token Definition
    /// - LP Lock Holding Account, derived as `compute_lp_lock_holding_pda(amm_program_id,
    ///   pool.account_id)`
    /// - User Holding Account for Token A (authorized)
    /// - User Holding Account for Token B (authorized)
    /// - User Holding Account for Pool Liquidity
    NewDefinition {
        token_a_amount: u128,
        token_b_amount: u128,
        amm_program_id: ProgramId,
    },

    /// Adds liquidity to the Pool
    ///
    /// Required accounts:
    /// - AMM Pool (initialized)
    /// - Vault Holding Account for Token A (initialized)
    /// - Vault Holding Account for Token B (initialized)
    /// - Pool Liquidity Token Definition (initialized)
    /// - User Holding Account for Token A (authorized)
    /// - User Holding Account for Token B (authorized)
    /// - User Holding Account for Pool Liquidity
    AddLiquidity {
        min_amount_liquidity: u128,
        max_amount_to_add_token_a: u128,
        max_amount_to_add_token_b: u128,
    },

    /// Removes liquidity from the Pool
    ///
    /// Required accounts:
    /// - AMM Pool (initialized)
    /// - Vault Holding Account for Token A (initialized)
    /// - Vault Holding Account for Token B (initialized)
    /// - Pool Liquidity Token Definition (initialized)
    /// - User Holding Account for Token A (initialized)
    /// - User Holding Account for Token B (initialized)
    /// - User Holding Account for Pool Liquidity (authorized)
    RemoveLiquidity {
        remove_liquidity_amount: u128,
        min_amount_to_remove_token_a: u128,
        min_amount_to_remove_token_b: u128,
    },

    /// Swap some quantity of Tokens (either Token A or Token B)
    /// while maintaining the Pool constant product.
    ///
    /// Required accounts:
    /// - AMM Pool (initialized)
    /// - Vault Holding Account for Token A (initialized)
    /// - Vault Holding Account for Token B (initialized)
    /// - User Holding Account for Token A
    /// - User Holding Account for Token B Either User Holding Account for Token A or Token B is
    ///   authorized.
    Swap {
        swap_amount_in: u128,
        min_amount_out: u128,
        token_definition_id_in: AccountId,
    },
}

pub const MINIMUM_LIQUIDITY: u128 = 1_000;

#[derive(Clone, Default, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct PoolDefinition {
    pub definition_token_a_id: AccountId,
    pub definition_token_b_id: AccountId,
    pub vault_a_id: AccountId,
    pub vault_b_id: AccountId,
    pub liquidity_pool_id: AccountId,
    pub liquidity_pool_supply: u128,
    pub reserve_a: u128,
    pub reserve_b: u128,
    /// Fees are currently not used
    pub fees: u128,
    /// Indicates whether the pool is initialized for use.
    /// `MINIMUM_LIQUIDITY` LP tokens are permanently locked at initialization
    /// and cannot be removed, so `liquidity_pool_supply` will never drop below
    /// `MINIMUM_LIQUIDITY` for pools created after the minimum-liquidity lock
    /// was introduced. Reaching that floor does not by itself imply
    /// `active = false`; pools may remain active with only the permanently
    /// locked minimum liquidity remaining.
    pub active: bool,
}

impl TryFrom<&Data> for PoolDefinition {
    type Error = std::io::Error;

    fn try_from(data: &Data) -> Result<Self, Self::Error> {
        PoolDefinition::try_from_slice(data.as_ref())
    }
}

impl From<&PoolDefinition> for Data {
    fn from(definition: &PoolDefinition) -> Self {
        // Using size_of_val as size hint for Vec allocation
        let mut data = Vec::with_capacity(std::mem::size_of_val(definition));

        BorshSerialize::serialize(definition, &mut data)
            .expect("Serialization to Vec should not fail");

        Data::try_from(data).expect("Token definition encoded data should fit into Data")
    }
}

pub fn compute_pool_pda(
    amm_program_id: ProgramId,
    definition_token_a_id: AccountId,
    definition_token_b_id: AccountId,
) -> AccountId {
    AccountId::from((
        &amm_program_id,
        &compute_pool_pda_seed(definition_token_a_id, definition_token_b_id),
    ))
}

pub fn compute_pool_pda_seed(
    definition_token_a_id: AccountId,
    definition_token_b_id: AccountId,
) -> PdaSeed {
    use risc0_zkvm::sha::{Impl, Sha256};

    let (token_1, token_2) = match definition_token_a_id
        .value()
        .cmp(definition_token_b_id.value())
    {
        std::cmp::Ordering::Less => (definition_token_b_id, definition_token_a_id),
        std::cmp::Ordering::Greater => (definition_token_a_id, definition_token_b_id),
        std::cmp::Ordering::Equal => panic!("Definitions match"),
    };

    let mut bytes = [0; 64];
    bytes[0..32].copy_from_slice(&token_1.to_bytes());
    bytes[32..].copy_from_slice(&token_2.to_bytes());

    PdaSeed::new(
        Impl::hash_bytes(&bytes)
            .as_bytes()
            .try_into()
            .expect("Hash output must be exactly 32 bytes long"),
    )
}

pub fn compute_vault_pda(
    amm_program_id: ProgramId,
    pool_id: AccountId,
    definition_token_id: AccountId,
) -> AccountId {
    AccountId::from((
        &amm_program_id,
        &compute_vault_pda_seed(pool_id, definition_token_id),
    ))
}

pub fn compute_vault_pda_seed(pool_id: AccountId, definition_token_id: AccountId) -> PdaSeed {
    use risc0_zkvm::sha::{Impl, Sha256};

    let mut bytes = [0; 64];
    bytes[0..32].copy_from_slice(&pool_id.to_bytes());
    bytes[32..].copy_from_slice(&definition_token_id.to_bytes());

    PdaSeed::new(
        Impl::hash_bytes(&bytes)
            .as_bytes()
            .try_into()
            .expect("Hash output must be exactly 32 bytes long"),
    )
}

pub fn compute_liquidity_token_pda(amm_program_id: ProgramId, pool_id: AccountId) -> AccountId {
    AccountId::from((&amm_program_id, &compute_liquidity_token_pda_seed(pool_id)))
}

pub fn compute_liquidity_token_pda_seed(pool_id: AccountId) -> PdaSeed {
    use risc0_zkvm::sha::{Impl, Sha256};

    let mut bytes = [0; 64];
    bytes[0..32].copy_from_slice(&pool_id.to_bytes());
    bytes[32..].copy_from_slice(&LIQUIDITY_TOKEN_PDA_SEED);

    PdaSeed::new(
        Impl::hash_bytes(&bytes)
            .as_bytes()
            .try_into()
            .expect("Hash output must be exactly 32 bytes long"),
    )
}

pub fn compute_lp_lock_holding_pda(amm_program_id: ProgramId, pool_id: AccountId) -> AccountId {
    AccountId::from((&amm_program_id, &compute_lp_lock_holding_pda_seed(pool_id)))
}

pub fn compute_lp_lock_holding_pda_seed(pool_id: AccountId) -> PdaSeed {
    use risc0_zkvm::sha::{Impl, Sha256};

    let mut bytes = [0; 64];
    bytes[0..32].copy_from_slice(&pool_id.to_bytes());
    bytes[32..].copy_from_slice(&LP_LOCK_HOLDING_PDA_SEED);

    PdaSeed::new(
        Impl::hash_bytes(&bytes)
            .as_bytes()
            .try_into()
            .expect("Hash output must be exactly 32 bytes long"),
    )
}
