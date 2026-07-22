//! Deterministic AMM account discovery and pair lifecycle inspection.
//!
//! These functions derive the complete protocol read set, then validate caller-supplied snapshots.
//! They perform no RPC, signing, submission, or deployed-program compatibility lookup.

use amm_core::{
    canonical_token_pair, compute_config_pda, compute_liquidity_token_pda,
    compute_lp_lock_holding_pda, compute_pool_pda, compute_vault_pda, spot_price_q64_64,
    MINIMUM_LIQUIDITY,
};
use amm_program::quote as program_quote;
use clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID;
use nssa_core::{
    account::{Account, AccountId},
    program::ProgramId,
};
use token_core::TokenHolding;
use twap_oracle_core::{compute_current_tick_account_pda, CurrentTickAccount};

use crate::{
    plan::AmmContext,
    quote::{
        AccountSnapshot, ValidatedFungibleDefinition, ValidatedFungibleHolding,
        ValidatedPoolSnapshot,
    },
    ClientError,
};

/// Deterministic pre-pool token order used by AMM pool PDA derivation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CanonicalPair {
    token_a_id: AccountId,
    token_b_id: AccountId,
}

impl CanonicalPair {
    /// Returns canonical token A, whose raw account-ID bytes sort after token B.
    #[must_use]
    pub const fn token_a_id(&self) -> AccountId {
        self.token_a_id
    }

    /// Returns canonical token B.
    #[must_use]
    pub const fn token_b_id(&self) -> AccountId {
        self.token_b_id
    }
}

/// One caller-named token definition and its deterministic pool vault.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TokenReadManifest {
    definition_id: AccountId,
    vault_id: AccountId,
}

impl TokenReadManifest {
    /// Returns the token definition account ID.
    #[must_use]
    pub const fn definition_id(&self) -> AccountId {
        self.definition_id
    }

    /// Returns the pool vault derived for this token definition.
    #[must_use]
    pub const fn vault_id(&self) -> AccountId {
        self.vault_id
    }
}

/// Complete deterministic account read set for inspecting a token pair.
///
/// `first_token` and `second_token` preserve caller order. Their vaults are therefore named by
/// token rather than by stored pool A/B order, which is unavailable until the pool is decoded.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PairReadManifest {
    canonical_pair: CanonicalPair,
    first_token: TokenReadManifest,
    second_token: TokenReadManifest,
    config_id: AccountId,
    pool_id: AccountId,
    liquidity_definition_id: AccountId,
    lp_lock_holding_id: AccountId,
    current_tick_id: AccountId,
    clock_id: AccountId,
}

impl PairReadManifest {
    /// Returns deterministic pre-pool token order.
    #[must_use]
    pub const fn canonical_pair(&self) -> CanonicalPair {
        self.canonical_pair
    }

    /// Returns caller's first token and its derived vault.
    #[must_use]
    pub const fn first_token(&self) -> TokenReadManifest {
        self.first_token
    }

    /// Returns caller's second token and its derived vault.
    #[must_use]
    pub const fn second_token(&self) -> TokenReadManifest {
        self.second_token
    }

    /// Returns singleton AMM config account ID.
    #[must_use]
    pub const fn config_id(&self) -> AccountId {
        self.config_id
    }

    /// Returns pair pool account ID.
    #[must_use]
    pub const fn pool_id(&self) -> AccountId {
        self.pool_id
    }

    /// Returns deterministic LP token definition account ID.
    #[must_use]
    pub const fn liquidity_definition_id(&self) -> AccountId {
        self.liquidity_definition_id
    }

    /// Returns deterministic permanently locked LP holding account ID.
    #[must_use]
    pub const fn lp_lock_holding_id(&self) -> AccountId {
        self.lp_lock_holding_id
    }

    /// Returns pool's TWAP current-tick account ID.
    #[must_use]
    pub const fn current_tick_id(&self) -> AccountId {
        self.current_tick_id
    }

    /// Returns canonical one-block clock account ID.
    #[must_use]
    pub const fn clock_id(&self) -> AccountId {
        self.clock_id
    }

    /// Looks up a derived vault by token definition ID.
    #[must_use]
    pub fn vault_id_for(&self, definition_id: AccountId) -> Option<AccountId> {
        if definition_id == self.first_token.definition_id {
            Some(self.first_token.vault_id)
        } else if definition_id == self.second_token.definition_id {
            Some(self.second_token.vault_id)
        } else {
            None
        }
    }
}

/// Snapshots fetched from a [`PairReadManifest`].
#[derive(Clone, Copy)]
pub struct PairReadSnapshots<'a> {
    pub pool: &'a AccountSnapshot,
    pub first_token_definition: &'a AccountSnapshot,
    pub second_token_definition: &'a AccountSnapshot,
    pub first_token_vault: &'a AccountSnapshot,
    pub second_token_vault: &'a AccountSnapshot,
    pub liquidity_definition: &'a AccountSnapshot,
    pub lp_lock_holding: &'a AccountSnapshot,
    pub current_tick: &'a AccountSnapshot,
    pub clock: &'a AccountSnapshot,
}

/// Validated canonical clock values used by current AMM instructions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ValidatedClockSnapshot {
    block_id: u64,
    timestamp: u64,
}

impl ValidatedClockSnapshot {
    #[must_use]
    pub const fn block_id(&self) -> u64 {
        self.block_id
    }

    #[must_use]
    pub const fn timestamp(&self) -> u64 {
        self.timestamp
    }
}

/// State of a derived vault before its pool exists.
///
/// Pool creation's chained Token Program transfer accepts either a default destination or an
/// existing fungible holding for the same definition. It does not require every derived vault to
/// be default merely because the pool account is default.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MissingVaultState {
    Uninitialized,
    ExistingFungible { balance: u128 },
}

/// Validated view of a pair whose pool account is still uninitialized.
#[derive(Clone)]
pub struct MissingPairInspection {
    manifest: PairReadManifest,
    first_token_definition: ValidatedFungibleDefinition,
    second_token_definition: ValidatedFungibleDefinition,
    first_vault: MissingVaultState,
    second_vault: MissingVaultState,
    clock: ValidatedClockSnapshot,
}

impl MissingPairInspection {
    #[must_use]
    pub const fn manifest(&self) -> PairReadManifest {
        self.manifest
    }

    #[must_use]
    pub const fn first_token_definition(&self) -> &ValidatedFungibleDefinition {
        &self.first_token_definition
    }

    #[must_use]
    pub const fn second_token_definition(&self) -> &ValidatedFungibleDefinition {
        &self.second_token_definition
    }

    #[must_use]
    pub const fn first_vault(&self) -> MissingVaultState {
        self.first_vault
    }

    #[must_use]
    pub const fn second_vault(&self) -> MissingVaultState {
        self.second_vault
    }

    #[must_use]
    pub const fn clock(&self) -> ValidatedClockSnapshot {
        self.clock
    }
}

/// Validated current view of an initialized pair.
#[derive(Clone)]
pub struct ActivePairInspection {
    manifest: PairReadManifest,
    caller_order: program_quote::PairOrder,
    pool: ValidatedPoolSnapshot,
    lp_lock_holding: ValidatedFungibleHolding,
    stored_spot_price_q64_64: u128,
    current_tick: CurrentTickAccount,
    clock: ValidatedClockSnapshot,
}

impl ActivePairInspection {
    #[must_use]
    pub const fn manifest(&self) -> PairReadManifest {
        self.manifest
    }

    /// Returns caller first/second order relative to stored pool A/B order.
    #[must_use]
    pub const fn caller_order(&self) -> program_quote::PairOrder {
        self.caller_order
    }

    /// Returns complete validated stored pool, token-definition, LP-definition, and vault state.
    #[must_use]
    pub const fn pool(&self) -> &ValidatedPoolSnapshot {
        &self.pool
    }

    /// Returns the validated holding containing permanently locked minimum liquidity.
    #[must_use]
    pub const fn lp_lock_holding(&self) -> &ValidatedFungibleHolding {
        &self.lp_lock_holding
    }

    /// Returns spot price from stored pool reserves as Q64.64 token B per token A.
    #[must_use]
    pub const fn stored_spot_price_q64_64(&self) -> u128 {
        self.stored_spot_price_q64_64
    }

    #[must_use]
    pub const fn current_tick(&self) -> &CurrentTickAccount {
        &self.current_tick
    }

    #[must_use]
    pub const fn clock(&self) -> ValidatedClockSnapshot {
        self.clock
    }
}

/// Current lifecycle state for a fully inspected pair read set.
#[derive(Clone)]
pub enum PairInspection {
    Missing(Box<MissingPairInspection>),
    Active(Box<ActivePairInspection>),
}

/// Derives the singleton config account without reading network state.
#[must_use]
pub fn derive_config_id(amm_program_id: ProgramId) -> AccountId {
    compute_config_pda(amm_program_id)
}

/// Validates and decodes an AMM config snapshot.
///
/// The program ID is accepted optimistically as the configured transaction target and PDA
/// namespace. This performs no release, ImageID, or deployment-version check.
pub fn inspect_config(
    amm_program_id: ProgramId,
    config_snapshot: &AccountSnapshot,
) -> Result<AmmContext, ClientError> {
    AmmContext::from_config_account(amm_program_id, config_snapshot)
}

/// Resolves deterministic pre-pool token order through the same helper used by pool PDA derivation.
pub fn canonical_pair(
    first_token_id: AccountId,
    second_token_id: AccountId,
) -> Result<CanonicalPair, ClientError> {
    let Some((token_a_id, token_b_id)) = canonical_token_pair(first_token_id, second_token_id)
    else {
        return Err(ClientError::IdenticalTokenDefinitions);
    };
    Ok(CanonicalPair {
        token_a_id,
        token_b_id,
    })
}

/// Derives every protocol account needed to inspect a caller-ordered pair.
pub fn derive_pair_read_manifest(
    context: &AmmContext,
    first_token_id: AccountId,
    second_token_id: AccountId,
) -> Result<PairReadManifest, ClientError> {
    let canonical_pair = canonical_pair(first_token_id, second_token_id)?;
    let pool_id = compute_pool_pda(
        context.amm_program_id,
        canonical_pair.token_a_id,
        canonical_pair.token_b_id,
    );

    Ok(PairReadManifest {
        canonical_pair,
        first_token: TokenReadManifest {
            definition_id: first_token_id,
            vault_id: compute_vault_pda(context.amm_program_id, pool_id, first_token_id),
        },
        second_token: TokenReadManifest {
            definition_id: second_token_id,
            vault_id: compute_vault_pda(context.amm_program_id, pool_id, second_token_id),
        },
        config_id: context.config_id(),
        pool_id,
        liquidity_definition_id: compute_liquidity_token_pda(context.amm_program_id, pool_id),
        lp_lock_holding_id: compute_lp_lock_holding_pda(context.amm_program_id, pool_id),
        current_tick_id: compute_current_tick_account_pda(
            context.twap_oracle_program_id(),
            pool_id,
        ),
        clock_id: CLOCK_01_PROGRAM_ACCOUNT_ID,
    })
}

/// Validates a pair read set and classifies its pool as missing or active.
pub fn inspect_pair(
    context: &AmmContext,
    first_token_id: AccountId,
    second_token_id: AccountId,
    snapshots: PairReadSnapshots<'_>,
) -> Result<PairInspection, ClientError> {
    let manifest = derive_pair_read_manifest(context, first_token_id, second_token_id)?;
    validate_snapshot_ids(manifest, &snapshots)?;

    let first_token_definition =
        ValidatedFungibleDefinition::new(context, snapshots.first_token_definition)?;
    let second_token_definition =
        ValidatedFungibleDefinition::new(context, snapshots.second_token_definition)?;
    let clock = validate_clock(snapshots.clock)?;

    if snapshots.pool.account() == &Account::default() {
        validate_uninitialized("liquidity definition", snapshots.liquidity_definition)?;
        validate_uninitialized("LP lock holding", snapshots.lp_lock_holding)?;
        validate_uninitialized("current tick", snapshots.current_tick)?;

        return Ok(PairInspection::Missing(Box::new(MissingPairInspection {
            manifest,
            first_vault: validate_missing_vault(
                "first token vault",
                snapshots.first_token_vault,
                context.token_program_id(),
                first_token_id,
            )?,
            second_vault: validate_missing_vault(
                "second token vault",
                snapshots.second_token_vault,
                context.token_program_id(),
                second_token_id,
            )?,
            first_token_definition,
            second_token_definition,
            clock,
        })));
    }

    let stored_pool =
        amm_core::PoolDefinition::try_from(&snapshots.pool.account().data).map_err(|_| {
            ClientError::InvalidAccountData {
                account: "AMM pool",
                expected: "PoolDefinition",
            }
        })?;
    let caller_order = program_quote::pair_order(&stored_pool, first_token_id, second_token_id)?;
    let (token_a_definition, token_b_definition, vault_a, vault_b) = match caller_order {
        program_quote::PairOrder::Stored => (
            snapshots.first_token_definition,
            snapshots.second_token_definition,
            snapshots.first_token_vault,
            snapshots.second_token_vault,
        ),
        program_quote::PairOrder::Reversed => (
            snapshots.second_token_definition,
            snapshots.first_token_definition,
            snapshots.second_token_vault,
            snapshots.first_token_vault,
        ),
    };
    let pool = ValidatedPoolSnapshot::new(
        context,
        snapshots.pool,
        token_a_definition,
        token_b_definition,
        vault_a,
        vault_b,
        snapshots.liquidity_definition,
    )?;

    // Reuse program-owned state validation for fee, minimum LP supply, and vault/reserve
    // consistency. Donations are intentionally allowed and remain visible in vault balances.
    let _ = crate::quote::sync_reserves(&pool)?;
    if pool.pool().reserve_a == 0 || pool.pool().reserve_b == 0 {
        return Err(ClientError::Quote {
            code: "reserve_zero",
            message: "Reserves must be nonzero",
        });
    }
    let lp_lock_holding = ValidatedFungibleHolding::new(
        context,
        snapshots.lp_lock_holding,
        pool.liquidity_definition(),
    )?;
    if lp_lock_holding.balance() < MINIMUM_LIQUIDITY {
        return Err(ClientError::InvalidAccountData {
            account: "LP lock holding",
            expected: "fungible LP holding with at least the permanently locked minimum liquidity",
        });
    }

    if snapshots.current_tick.account().program_owner != context.twap_oracle_program_id() {
        return Err(ClientError::ProgramOwnerMismatch {
            account: "current tick",
            expected: context.twap_oracle_program_id(),
            actual: snapshots.current_tick.account().program_owner,
        });
    }
    let current_tick = CurrentTickAccount::try_from(&snapshots.current_tick.account().data)
        .map_err(|_| ClientError::InvalidAccountData {
            account: "current tick",
            expected: "CurrentTickAccount",
        })?;
    let stored_spot_price_q64_64 = spot_price_q64_64(pool.pool().reserve_a, pool.pool().reserve_b);

    Ok(PairInspection::Active(Box::new(ActivePairInspection {
        manifest,
        caller_order,
        pool,
        lp_lock_holding,
        stored_spot_price_q64_64,
        current_tick,
        clock,
    })))
}

fn validate_snapshot_ids(
    manifest: PairReadManifest,
    snapshots: &PairReadSnapshots<'_>,
) -> Result<(), ClientError> {
    for (name, snapshot, expected) in [
        ("pool", snapshots.pool, manifest.pool_id),
        (
            "first token definition",
            snapshots.first_token_definition,
            manifest.first_token.definition_id,
        ),
        (
            "second token definition",
            snapshots.second_token_definition,
            manifest.second_token.definition_id,
        ),
        (
            "first token vault",
            snapshots.first_token_vault,
            manifest.first_token.vault_id,
        ),
        (
            "second token vault",
            snapshots.second_token_vault,
            manifest.second_token.vault_id,
        ),
        (
            "liquidity definition",
            snapshots.liquidity_definition,
            manifest.liquidity_definition_id,
        ),
        (
            "LP lock holding",
            snapshots.lp_lock_holding,
            manifest.lp_lock_holding_id,
        ),
        (
            "current tick",
            snapshots.current_tick,
            manifest.current_tick_id,
        ),
        ("clock", snapshots.clock, manifest.clock_id),
    ] {
        if snapshot.account_id() != expected {
            return Err(ClientError::AccountIdMismatch {
                account: name,
                expected,
                actual: snapshot.account_id(),
            });
        }
    }
    Ok(())
}

fn validate_uninitialized(
    account_name: &'static str,
    snapshot: &AccountSnapshot,
) -> Result<(), ClientError> {
    if snapshot.account() != &Account::default() {
        return Err(ClientError::InvalidAccountData {
            account: account_name,
            expected: "uninitialized account",
        });
    }
    Ok(())
}

fn validate_missing_vault(
    account_name: &'static str,
    snapshot: &AccountSnapshot,
    token_program_id: ProgramId,
    expected_definition_id: AccountId,
) -> Result<MissingVaultState, ClientError> {
    if snapshot.account() == &Account::default() {
        return Ok(MissingVaultState::Uninitialized);
    }

    if snapshot.account().program_owner != token_program_id {
        return Err(ClientError::ProgramOwnerMismatch {
            account: account_name,
            expected: token_program_id,
            actual: snapshot.account().program_owner,
        });
    }

    // Existing recipients must be writable by the Token Program. A default destination remains
    // valid because the chained transfer claims it for the Token Program.
    let holding = TokenHolding::try_from(&snapshot.account().data).map_err(|_| {
        ClientError::InvalidAccountData {
            account: account_name,
            expected: "TokenHolding",
        }
    })?;
    let TokenHolding::Fungible {
        definition_id,
        balance,
    } = holding
    else {
        return Err(ClientError::ExpectedFungibleToken {
            account: account_name,
        });
    };
    if definition_id != expected_definition_id {
        return Err(ClientError::TokenDefinitionMismatch {
            account: account_name,
            expected: expected_definition_id,
            actual: definition_id,
        });
    }

    Ok(MissingVaultState::ExistingFungible { balance })
}

fn validate_clock(snapshot: &AccountSnapshot) -> Result<ValidatedClockSnapshot, ClientError> {
    let bytes = snapshot.account().data.as_ref();
    if bytes.len() != 16 {
        return Err(ClientError::InvalidAccountData {
            account: "clock",
            expected: "ClockAccountData",
        });
    }
    let (block_id_bytes, timestamp_bytes) = bytes.split_at(8);
    let block_id = u64::from_le_bytes(block_id_bytes.try_into().map_err(|_| {
        ClientError::InvalidAccountData {
            account: "clock",
            expected: "ClockAccountData",
        }
    })?);
    let timestamp = u64::from_le_bytes(timestamp_bytes.try_into().map_err(|_| {
        ClientError::InvalidAccountData {
            account: "clock",
            expected: "ClockAccountData",
        }
    })?);

    Ok(ValidatedClockSnapshot {
        block_id,
        timestamp,
    })
}
