//! Validated account snapshots and high-level AMM quote orchestration.
//!
//! This module validates fetched protocol accounts, then delegates every economic calculation to
//! [`amm_program::quote`]. It performs no RPC, signing, submission, floating-point conversion, or
//! runtime program-version check.

use amm_core::{compute_config_pda, AmmConfig, PoolDefinition};
use amm_program::quote as program_quote;
use nssa_core::{
    account::{Account, AccountId},
    program::ProgramId,
};
use token_core::{TokenDefinition, TokenHolding};

use crate::{AmmContext, ClientError, PoolContext};

/// An immutable fetched account paired with the ID used to fetch it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountSnapshot {
    account_id: AccountId,
    account: Account,
}

impl AccountSnapshot {
    /// Creates an account snapshot from canonical NSSA account data.
    #[must_use]
    pub fn new(account_id: AccountId, account: Account) -> Self {
        Self {
            account_id,
            account,
        }
    }

    /// Returns the fetched account ID.
    #[must_use]
    pub const fn account_id(&self) -> AccountId {
        self.account_id
    }

    /// Returns the fetched canonical account.
    #[must_use]
    pub const fn account(&self) -> &Account {
        &self.account
    }
}

impl AmmContext {
    /// Validates and decodes the singleton config account for the supplied AMM program ID.
    ///
    /// The supplied program ID is used optimistically. This checks protocol ownership and the
    /// config PDA, but intentionally performs no ImageID, version, or build-compatibility lookup.
    pub fn from_config_account(
        amm_program_id: ProgramId,
        config_account: &AccountSnapshot,
    ) -> Result<Self, ClientError> {
        ensure_account_id(
            "AMM config",
            config_account,
            compute_config_pda(amm_program_id),
        )?;
        ensure_program_owner("AMM config", config_account, amm_program_id)?;
        let config = AmmConfig::try_from(&config_account.account.data).map_err(|_| {
            ClientError::InvalidAccountData {
                account: "AMM config",
                expected: "AmmConfig",
            }
        })?;

        Ok(Self::new(amm_program_id, config))
    }
}

/// A token definition proven to be a configured-token-program fungible definition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedFungibleDefinition {
    account_id: AccountId,
    token_program_id: ProgramId,
    total_supply: u128,
    authority: Option<AccountId>,
}

impl ValidatedFungibleDefinition {
    /// Validates a fungible token definition account against an AMM context.
    pub fn new(
        context: &AmmContext,
        definition_account: &AccountSnapshot,
    ) -> Result<Self, ClientError> {
        ensure_program_owner(
            "token definition",
            definition_account,
            context.token_program_id(),
        )?;
        let definition =
            TokenDefinition::try_from(&definition_account.account.data).map_err(|_| {
                ClientError::InvalidAccountData {
                    account: "token definition",
                    expected: "TokenDefinition",
                }
            })?;
        let TokenDefinition::Fungible {
            total_supply,
            authority,
            ..
        } = definition
        else {
            return Err(ClientError::ExpectedFungibleToken {
                account: "token definition",
            });
        };

        Ok(Self {
            account_id: definition_account.account_id,
            token_program_id: context.token_program_id(),
            total_supply,
            authority,
        })
    }

    /// Returns the token definition account ID.
    #[must_use]
    pub const fn account_id(&self) -> AccountId {
        self.account_id
    }

    /// Returns the exact raw supply stored by the token program.
    #[must_use]
    pub const fn total_supply(&self) -> u128 {
        self.total_supply
    }

    /// Returns the token definition's current mint authority.
    #[must_use]
    pub const fn authority(&self) -> Option<AccountId> {
        self.authority
    }
}

/// A token holding proven to be fungible, configured-token-program owned, and tied to an expected
/// definition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedFungibleHolding {
    account_id: AccountId,
    definition_id: AccountId,
    balance: u128,
    token_program_id: ProgramId,
}

impl ValidatedFungibleHolding {
    /// Validates a fungible holding against an expected token definition.
    pub fn new(
        context: &AmmContext,
        holding_account: &AccountSnapshot,
        expected_definition: &ValidatedFungibleDefinition,
    ) -> Result<Self, ClientError> {
        ensure_definition_context(context, expected_definition, "expected token definition")?;
        Self::for_definition_id(context, holding_account, expected_definition.account_id)
    }

    fn for_definition_id(
        context: &AmmContext,
        holding_account: &AccountSnapshot,
        expected_definition_id: AccountId,
    ) -> Result<Self, ClientError> {
        ensure_program_owner("token holding", holding_account, context.token_program_id())?;
        let holding = TokenHolding::try_from(&holding_account.account.data).map_err(|_| {
            ClientError::InvalidAccountData {
                account: "token holding",
                expected: "TokenHolding",
            }
        })?;
        let TokenHolding::Fungible {
            definition_id,
            balance,
        } = holding
        else {
            return Err(ClientError::ExpectedFungibleToken {
                account: "token holding",
            });
        };
        if definition_id != expected_definition_id {
            return Err(ClientError::TokenDefinitionMismatch {
                account: "token holding",
                expected: expected_definition_id,
                actual: definition_id,
            });
        }

        Ok(Self {
            account_id: holding_account.account_id,
            definition_id,
            balance,
            token_program_id: context.token_program_id(),
        })
    }

    /// Returns the holding account ID.
    #[must_use]
    pub const fn account_id(&self) -> AccountId {
        self.account_id
    }

    /// Returns the held token definition account ID.
    #[must_use]
    pub const fn definition_id(&self) -> AccountId {
        self.definition_id
    }

    /// Returns the exact raw fungible balance.
    #[must_use]
    pub const fn balance(&self) -> u128 {
        self.balance
    }
}

/// A decoded pool whose owner, PDA, stored account IDs, vault holdings, and fungible token
/// definitions have been validated together.
#[derive(Clone)]
pub struct ValidatedPoolSnapshot {
    pool_id: AccountId,
    pool: PoolDefinition,
    token_a_definition: ValidatedFungibleDefinition,
    token_b_definition: ValidatedFungibleDefinition,
    liquidity_definition: ValidatedFungibleDefinition,
    vault_a: ValidatedFungibleHolding,
    vault_b: ValidatedFungibleHolding,
}

impl ValidatedPoolSnapshot {
    /// Validates a complete initialized pool snapshot.
    pub fn new(
        context: &AmmContext,
        pool_account: &AccountSnapshot,
        token_a_definition_account: &AccountSnapshot,
        token_b_definition_account: &AccountSnapshot,
        vault_a_account: &AccountSnapshot,
        vault_b_account: &AccountSnapshot,
        liquidity_definition_account: &AccountSnapshot,
    ) -> Result<Self, ClientError> {
        ensure_program_owner("AMM pool", pool_account, context.amm_program_id)?;
        let pool = PoolDefinition::try_from(&pool_account.account.data).map_err(|_| {
            ClientError::InvalidAccountData {
                account: "AMM pool",
                expected: "PoolDefinition",
            }
        })?;
        if pool.definition_token_a_id == pool.definition_token_b_id {
            return Err(ClientError::IdenticalTokenDefinitions);
        }
        PoolContext::new(context, pool_account.account_id, &pool)?;

        let token_a_definition =
            ValidatedFungibleDefinition::new(context, token_a_definition_account)?;
        ensure_definition_id(
            "token A definition",
            &token_a_definition,
            pool.definition_token_a_id,
        )?;
        let token_b_definition =
            ValidatedFungibleDefinition::new(context, token_b_definition_account)?;
        ensure_definition_id(
            "token B definition",
            &token_b_definition,
            pool.definition_token_b_id,
        )?;
        let liquidity_definition =
            ValidatedFungibleDefinition::new(context, liquidity_definition_account)?;
        ensure_definition_id(
            "liquidity definition",
            &liquidity_definition,
            pool.liquidity_pool_id,
        )?;
        if liquidity_definition.total_supply != pool.liquidity_pool_supply {
            return Err(ClientError::InvalidAccountData {
                account: "liquidity definition",
                expected: "fungible LP definition with supply equal to pool liquidity supply",
            });
        }
        if liquidity_definition.authority != Some(pool.liquidity_pool_id) {
            return Err(ClientError::InvalidAccountData {
                account: "liquidity definition",
                expected: "self-authorized fungible LP definition",
            });
        }

        ensure_account_id("vault A", vault_a_account, pool.vault_a_id)?;
        ensure_account_id("vault B", vault_b_account, pool.vault_b_id)?;
        let vault_a = ValidatedFungibleHolding::for_definition_id(
            context,
            vault_a_account,
            pool.definition_token_a_id,
        )?;
        let vault_b = ValidatedFungibleHolding::for_definition_id(
            context,
            vault_b_account,
            pool.definition_token_b_id,
        )?;

        Ok(Self {
            pool_id: pool_account.account_id,
            pool,
            token_a_definition,
            token_b_definition,
            liquidity_definition,
            vault_a,
            vault_b,
        })
    }

    /// Returns the pool account ID.
    #[must_use]
    pub const fn pool_id(&self) -> AccountId {
        self.pool_id
    }

    /// Returns the decoded pool state.
    #[must_use]
    pub const fn pool(&self) -> &PoolDefinition {
        &self.pool
    }

    /// Returns the validated token-A definition.
    #[must_use]
    pub const fn token_a_definition(&self) -> &ValidatedFungibleDefinition {
        &self.token_a_definition
    }

    /// Returns the validated token-B definition.
    #[must_use]
    pub const fn token_b_definition(&self) -> &ValidatedFungibleDefinition {
        &self.token_b_definition
    }

    /// Returns the validated liquidity-token definition.
    #[must_use]
    pub const fn liquidity_definition(&self) -> &ValidatedFungibleDefinition {
        &self.liquidity_definition
    }

    /// Returns the validated token-A vault holding.
    #[must_use]
    pub const fn vault_a(&self) -> &ValidatedFungibleHolding {
        &self.vault_a
    }

    /// Returns the validated token-B vault holding.
    #[must_use]
    pub const fn vault_b(&self) -> &ValidatedFungibleHolding {
        &self.vault_b
    }
}

impl From<program_quote::QuoteError> for ClientError {
    fn from(error: program_quote::QuoteError) -> Self {
        Self::Quote {
            code: error.code(),
            message: error.message(),
        }
    }
}

/// Resolves caller token order against a validated pool.
pub fn pair_order(
    snapshot: &ValidatedPoolSnapshot,
    first_token: &ValidatedFungibleDefinition,
    second_token: &ValidatedFungibleDefinition,
) -> Result<program_quote::PairOrder, ClientError> {
    Ok(program_quote::pair_order(
        &snapshot.pool,
        first_token.account_id,
        second_token.account_id,
    )?)
}

/// Quotes initial pool liquidity for two validated fungible definitions.
pub fn create_pool(
    context: &AmmContext,
    token_a: &ValidatedFungibleDefinition,
    token_b: &ValidatedFungibleDefinition,
    token_a_amount: u128,
    token_b_amount: u128,
    fee_bps: u128,
) -> Result<program_quote::CreatePoolQuote, ClientError> {
    ensure_definition_context(context, token_a, "token A definition")?;
    ensure_definition_context(context, token_b, "token B definition")?;
    if token_a.account_id == token_b.account_id {
        return Err(ClientError::IdenticalTokenDefinitions);
    }

    Ok(program_quote::create_pool(
        token_a_amount,
        token_b_amount,
        fee_bps,
    )?)
}

/// Previews an add-liquidity transition from validated pool and vault state.
pub fn preview_add_liquidity(
    snapshot: &ValidatedPoolSnapshot,
    max_amount_a: u128,
    max_amount_b: u128,
) -> Result<program_quote::AddLiquidityQuote, ClientError> {
    Ok(program_quote::preview_add_liquidity(
        &snapshot.pool,
        snapshot.vault_a.balance,
        snapshot.vault_b.balance,
        max_amount_a,
        max_amount_b,
    )?)
}

/// Quotes an add-liquidity transition with the exact execution guard.
pub fn add_liquidity(
    snapshot: &ValidatedPoolSnapshot,
    max_amount_a: u128,
    max_amount_b: u128,
    minimum_liquidity: u128,
) -> Result<program_quote::AddLiquidityQuote, ClientError> {
    Ok(program_quote::add_liquidity(
        &snapshot.pool,
        snapshot.vault_a.balance,
        snapshot.vault_b.balance,
        max_amount_a,
        max_amount_b,
        minimum_liquidity,
    )?)
}

/// Previews a remove-liquidity transition using a validated LP holding.
pub fn preview_remove_liquidity(
    snapshot: &ValidatedPoolSnapshot,
    user_liquidity: &ValidatedFungibleHolding,
    remove_liquidity_amount: u128,
) -> Result<program_quote::RemoveLiquidityQuote, ClientError> {
    ensure_pool_holding(
        snapshot,
        user_liquidity,
        snapshot.pool.liquidity_pool_id,
        "user liquidity holding",
    )?;
    Ok(program_quote::preview_remove_liquidity(
        &snapshot.pool,
        user_liquidity.balance,
        remove_liquidity_amount,
    )?)
}

/// Quotes a remove-liquidity transition with the exact execution guards.
pub fn remove_liquidity(
    snapshot: &ValidatedPoolSnapshot,
    user_liquidity: &ValidatedFungibleHolding,
    remove_liquidity_amount: u128,
    minimum_amount_a: u128,
    minimum_amount_b: u128,
) -> Result<program_quote::RemoveLiquidityQuote, ClientError> {
    ensure_pool_holding(
        snapshot,
        user_liquidity,
        snapshot.pool.liquidity_pool_id,
        "user liquidity holding",
    )?;
    Ok(program_quote::remove_liquidity(
        &snapshot.pool,
        user_liquidity.balance,
        remove_liquidity_amount,
        minimum_amount_a,
        minimum_amount_b,
    )?)
}

/// Previews an exact-input swap, deriving direction from the validated input holding.
pub fn preview_swap_exact_input(
    snapshot: &ValidatedPoolSnapshot,
    user_input: &ValidatedFungibleHolding,
    user_output: &ValidatedFungibleHolding,
    amount_in: u128,
) -> Result<program_quote::SwapQuote, ClientError> {
    let direction = validated_swap_direction(snapshot, user_input, user_output)?;
    let quote = program_quote::preview_swap_exact_input(
        &snapshot.pool,
        snapshot.vault_a.balance,
        snapshot.vault_b.balance,
        direction,
        amount_in,
    )?;
    ensure_available_balance(user_input, quote.amount_in, "user input holding")?;
    Ok(quote)
}

/// Quotes an exact-input swap with its exact minimum-output guard.
pub fn swap_exact_input(
    snapshot: &ValidatedPoolSnapshot,
    user_input: &ValidatedFungibleHolding,
    user_output: &ValidatedFungibleHolding,
    amount_in: u128,
    minimum_amount_out: u128,
) -> Result<program_quote::SwapQuote, ClientError> {
    let direction = validated_swap_direction(snapshot, user_input, user_output)?;
    let quote = program_quote::swap_exact_input(
        &snapshot.pool,
        snapshot.vault_a.balance,
        snapshot.vault_b.balance,
        direction,
        amount_in,
        minimum_amount_out,
    )?;
    ensure_available_balance(user_input, quote.amount_in, "user input holding")?;
    Ok(quote)
}

/// Previews an exact-output swap, deriving direction from the validated input holding.
pub fn preview_swap_exact_output(
    snapshot: &ValidatedPoolSnapshot,
    user_input: &ValidatedFungibleHolding,
    user_output: &ValidatedFungibleHolding,
    exact_amount_out: u128,
) -> Result<program_quote::SwapQuote, ClientError> {
    let direction = validated_swap_direction(snapshot, user_input, user_output)?;
    let quote = program_quote::preview_swap_exact_output(
        &snapshot.pool,
        snapshot.vault_a.balance,
        snapshot.vault_b.balance,
        direction,
        exact_amount_out,
    )?;
    ensure_available_balance(user_input, quote.amount_in, "user input holding")?;
    Ok(quote)
}

/// Quotes an exact-output swap with its exact maximum-input guard.
pub fn swap_exact_output(
    snapshot: &ValidatedPoolSnapshot,
    user_input: &ValidatedFungibleHolding,
    user_output: &ValidatedFungibleHolding,
    exact_amount_out: u128,
    maximum_amount_in: u128,
) -> Result<program_quote::SwapQuote, ClientError> {
    let direction = validated_swap_direction(snapshot, user_input, user_output)?;
    let quote = program_quote::swap_exact_output(
        &snapshot.pool,
        snapshot.vault_a.balance,
        snapshot.vault_b.balance,
        direction,
        exact_amount_out,
        maximum_amount_in,
    )?;
    ensure_available_balance(user_input, quote.amount_in, "user input holding")?;
    Ok(quote)
}

/// Quotes reserve synchronization from validated pool and vault state.
pub fn sync_reserves(
    snapshot: &ValidatedPoolSnapshot,
) -> Result<program_quote::SyncReservesQuote, ClientError> {
    Ok(program_quote::sync_reserves(
        &snapshot.pool,
        snapshot.vault_a.balance,
        snapshot.vault_b.balance,
    )?)
}

/// Quotes pool-derived initialization values for an oracle price account.
pub fn create_oracle_price_account(
    snapshot: &ValidatedPoolSnapshot,
    window_duration: u64,
) -> Result<program_quote::OraclePriceAccountQuote, ClientError> {
    Ok(program_quote::create_oracle_price_account(
        &snapshot.pool,
        window_duration,
    )?)
}

fn validated_swap_direction(
    snapshot: &ValidatedPoolSnapshot,
    user_input: &ValidatedFungibleHolding,
    user_output: &ValidatedFungibleHolding,
) -> Result<program_quote::SwapDirection, ClientError> {
    ensure_holding_context(snapshot, user_input, "user input holding")?;
    ensure_holding_context(snapshot, user_output, "user output holding")?;
    let direction = program_quote::swap_direction(&snapshot.pool, user_input.definition_id)?;
    let expected_output_definition = match direction {
        program_quote::SwapDirection::AToB => snapshot.pool.definition_token_b_id,
        program_quote::SwapDirection::BToA => snapshot.pool.definition_token_a_id,
    };
    ensure_pool_holding(
        snapshot,
        user_output,
        expected_output_definition,
        "user output holding",
    )?;
    Ok(direction)
}

fn ensure_account_id(
    account_name: &'static str,
    snapshot: &AccountSnapshot,
    expected: AccountId,
) -> Result<(), ClientError> {
    if snapshot.account_id != expected {
        return Err(ClientError::AccountIdMismatch {
            account: account_name,
            expected,
            actual: snapshot.account_id,
        });
    }
    Ok(())
}

fn ensure_program_owner(
    account_name: &'static str,
    snapshot: &AccountSnapshot,
    expected: ProgramId,
) -> Result<(), ClientError> {
    if snapshot.account.program_owner != expected {
        return Err(ClientError::ProgramOwnerMismatch {
            account: account_name,
            expected,
            actual: snapshot.account.program_owner,
        });
    }
    Ok(())
}

fn ensure_definition_context(
    context: &AmmContext,
    definition: &ValidatedFungibleDefinition,
    account_name: &'static str,
) -> Result<(), ClientError> {
    if definition.token_program_id != context.token_program_id() {
        return Err(ClientError::ProgramOwnerMismatch {
            account: account_name,
            expected: context.token_program_id(),
            actual: definition.token_program_id,
        });
    }
    Ok(())
}

fn ensure_definition_id(
    account_name: &'static str,
    definition: &ValidatedFungibleDefinition,
    expected: AccountId,
) -> Result<(), ClientError> {
    if definition.account_id != expected {
        return Err(ClientError::TokenDefinitionMismatch {
            account: account_name,
            expected,
            actual: definition.account_id,
        });
    }
    Ok(())
}

fn ensure_holding_context(
    snapshot: &ValidatedPoolSnapshot,
    holding: &ValidatedFungibleHolding,
    account_name: &'static str,
) -> Result<(), ClientError> {
    if holding.token_program_id != snapshot.vault_a.token_program_id {
        return Err(ClientError::ProgramOwnerMismatch {
            account: account_name,
            expected: snapshot.vault_a.token_program_id,
            actual: holding.token_program_id,
        });
    }
    Ok(())
}

fn ensure_pool_holding(
    snapshot: &ValidatedPoolSnapshot,
    holding: &ValidatedFungibleHolding,
    expected_definition_id: AccountId,
    account_name: &'static str,
) -> Result<(), ClientError> {
    ensure_holding_context(snapshot, holding, account_name)?;
    if holding.definition_id != expected_definition_id {
        return Err(ClientError::TokenDefinitionMismatch {
            account: account_name,
            expected: expected_definition_id,
            actual: holding.definition_id,
        });
    }
    Ok(())
}

fn ensure_available_balance(
    holding: &ValidatedFungibleHolding,
    required: u128,
    account_name: &'static str,
) -> Result<(), ClientError> {
    if holding.balance < required {
        return Err(ClientError::InsufficientBalance {
            account: account_name,
            available: holding.balance,
            required,
        });
    }
    Ok(())
}
