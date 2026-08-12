//! This crate contains core data structures and utilities for the AMM Program.

use borsh::{BorshDeserialize, BorshSerialize};
use nssa_core::{
    account::{AccountId, AccountWithMetadata, Data},
    program::{PdaSeed, ProgramId},
};
use serde::{Deserialize, Serialize};
use spel_framework_macros::account_type;

// These stable domain-separation tags are part of the PDA derivation scheme and must stay
// unchanged for address compatibility.
const LIQUIDITY_TOKEN_PDA_SEED: &[u8] = b"LIQUIDITY_TOKEN";
const LP_LOCK_HOLDING_PDA_SEED: &[u8] = b"LP_LOCK_HOLDING";

/// AMM Program Instruction.
#[derive(Serialize, Deserialize)]
pub enum Instruction {
    /// Initializes the AMM Program by creating its singleton configuration account.
    ///
    /// The configuration account is a PDA derived from the constant `"CONFIG"` seed
    /// (`compute_config_pda(self_program_id)`). It stores the program IDs the AMM issues chained
    /// calls to (the Token Program and the TWAP oracle program), plus the admin `authority`
    /// allowed to transfer admin control later via `UpdateConfig`. The Program must be initialized
    /// via this instruction before any pool can be created or interacted with — the other
    /// instructions read these program IDs from this account and reject calls when it does not
    /// yet exist.
    ///
    /// Required accounts:
    /// - AMM Config Account, uninitialized, derived as `compute_config_pda(self_program_id)`
    Initialize {
        /// Program ID of the Token Program the AMM will issue chained calls to.
        token_program_id: ProgramId,
        /// Program ID of the TWAP oracle program the AMM will issue chained calls to.
        twap_oracle_program_id: ProgramId,
        /// Admin authority allowed to transfer admin control via `UpdateConfig`.
        authority: AccountId,
    },

    /// Transfers the AMM Program's admin authority to a new account. Only the configured admin
    /// `authority` may call this; the authority account must be passed authorized (signed).
    ///
    /// The Token Program and TWAP oracle program IDs are **immutable deployment parameters** set
    /// once at `Initialize`: they are baked into every derived PDA (vaults, current-tick,
    /// price-observation / price accounts) and into the AMM's chained calls, so changing them
    /// after any pool exists would orphan every derived account and vault. They therefore cannot
    /// be reconfigured in place — a genuine change requires redeploying the AMM. This instruction
    /// only moves the admin authority.
    ///
    /// Required accounts:
    /// - AMM Config Account (initialized)
    /// - Authority Account — must equal the config's current `authority`, passed authorized.
    UpdateConfig {
        /// New admin authority (transfers admin control to this account).
        new_authority: AccountId,
    },

    /// Creates a TWAP price-observations account for a pool over a time window, on behalf of the
    /// AMM, via a chained call to the configured TWAP oracle program.
    ///
    /// The pool acts as the price source: the AMM authorizes it (via its pool PDA seed) so the
    /// oracle ties the observations account to this pool. The feed's initial tick is read from the
    /// pool's [`CurrentTickAccount`](twap_oracle_core::CurrentTickAccount) — the authoritative
    /// tick the AMM previously wrote — rather than being supplied by the caller, so the feed
    /// cannot be seeded at a forged price. Rejects if the observations account already exists.
    /// The clock must be the canonical 1-block LEZ clock.
    ///
    /// Required accounts:
    /// - AMM Config Account (initialized)
    /// - AMM Pool (initialized; acts as the price source)
    /// - Current Tick Account, the pool's initialized TWAP PDA derived as
    ///   `compute_current_tick_account_pda(twap_oracle_program_id, pool.account_id)`; supplies the
    ///   initial tick
    /// - Price Observations Account, uninitialized TWAP PDA derived as
    ///   `compute_price_observations_pda(twap_oracle_program_id, pool.account_id,
    ///   window_duration)`
    /// - Clock Account (the canonical 1-block LEZ clock)
    CreatePriceObservations {
        /// Duration of the TWAP window this feed serves, in milliseconds. Part of the
        /// observations PDA seed, so each window gets a distinct account.
        window_duration: u64,
    },

    /// Creates a TWAP oracle price account for a pool over a time window, on behalf of the AMM,
    /// via a chained call to the configured TWAP oracle program.
    ///
    /// The pool acts as the price source: the AMM authorizes it (via its pool PDA seed) so the
    /// oracle ties the price account to this pool. The base/quote assets are the pool's token
    /// definitions and the initial price is the pool's current spot price
    /// (`reserve_b / reserve_a` as a Q64.64), read from the validated pool rather than supplied by
    /// the caller — so the account cannot be seeded at a forged price. The account is overwritten
    /// by `PublishPrice` once the feed has observations. Rejects if the price account already
    /// exists. The clock must be the canonical 1-block LEZ clock.
    ///
    /// Required accounts:
    /// - AMM Config Account (initialized)
    /// - AMM Pool (initialized; acts as the price source)
    /// - Oracle Price Account, uninitialized TWAP PDA derived as
    ///   `compute_oracle_price_account_pda(twap_oracle_program_id, pool.account_id,
    ///   window_duration)`
    /// - Clock Account (the canonical 1-block LEZ clock)
    CreateOraclePriceAccount {
        /// Duration of the TWAP window this price account serves, in milliseconds. Part of the
        /// price-account PDA seed, so each window gets a distinct account.
        window_duration: u64,
    },

    /// Initializes a new Pool (or re-initializes an existing zero-supply Pool).
    ///
    /// On initialization, `MINIMUM_LIQUIDITY` LP tokens are permanently locked
    /// in the LP-lock holding PDA; the caller receives `initial_lp - MINIMUM_LIQUIDITY`.
    ///
    /// Required accounts:
    /// - AMM Pool
    /// - Vault Holding Account for Token A
    /// - Vault Holding Account for Token B
    /// - Pool Liquidity Token Definition
    /// - LP Lock Holding Account, derived as `compute_lp_lock_holding_pda(self_program_id,
    ///   pool.account_id)`
    /// - User Holding Account for Token A (authorized)
    /// - User Holding Account for Token B (authorized)
    /// - User Holding Account for Pool Liquidity (authorized when uninitialized)
    NewDefinition {
        token_a_amount: u128,
        token_b_amount: u128,
        fees: u128,
        /// Unix timestamp (milliseconds) after which this transaction is invalid.
        deadline: u64,
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
    /// - Current Tick Account, the pool's TWAP PDA derived as
    ///   `compute_current_tick_account_pda(twap_oracle_program_id, pool.account_id)`; refreshed
    ///   with the new spot price
    /// - Clock Account (the canonical 1-block LEZ clock)
    AddLiquidity {
        min_amount_liquidity: u128,
        max_amount_to_add_token_a: u128,
        max_amount_to_add_token_b: u128,
        /// Unix timestamp (milliseconds) after which this transaction is invalid.
        deadline: u64,
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
    /// - Current Tick Account, the pool's TWAP PDA derived as
    ///   `compute_current_tick_account_pda(twap_oracle_program_id, pool.account_id)`; refreshed
    ///   with the new spot price
    /// - Clock Account (the canonical 1-block LEZ clock)
    RemoveLiquidity {
        remove_liquidity_amount: u128,
        min_amount_to_remove_token_a: u128,
        min_amount_to_remove_token_b: u128,
        /// Unix timestamp (milliseconds) after which this transaction is invalid.
        deadline: u64,
    },

    /// Swap some quantity of tokens while maintaining the Pool constant product.
    ///
    /// Swap direction is determined by the input holding: `user_input_holding`'s token definition
    /// selects which pool token is sold. That holding must be signed so the downstream token
    /// transfer can debit it; `user_output_holding` only receives and needs no signature.
    ///
    /// Required accounts:
    /// - AMM Pool (initialized)
    /// - Vault Holding Account for Token A (initialized)
    /// - Vault Holding Account for Token B (initialized)
    /// - User Input Holding Account (initialized, signed) — the token being sold
    /// - User Output Holding Account (initialized) — receives the token being bought
    /// - Current Tick Account, the pool's TWAP PDA derived as
    ///   `compute_current_tick_account_pda(twap_oracle_program_id, pool.account_id)`; refreshed
    ///   with the new spot price
    /// - Clock Account (the canonical 1-block LEZ clock)
    SwapExactInput {
        swap_amount_in: u128,
        min_amount_out: u128,
        /// Unix timestamp (milliseconds) after which this transaction is invalid.
        deadline: u64,
    },

    /// Swap tokens specifying the exact desired output amount while maintaining the Pool constant
    /// product.
    ///
    /// Swap direction is determined by the input holding: `user_input_holding`'s token definition
    /// selects which pool token is sold. That holding must be signed so the downstream token
    /// transfer can debit it; `user_output_holding` only receives and needs no signature.
    ///
    /// Required accounts:
    /// - AMM Pool (initialized)
    /// - Vault Holding Account for Token A (initialized)
    /// - Vault Holding Account for Token B (initialized)
    /// - User Input Holding Account (initialized, signed) — the token being sold
    /// - User Output Holding Account (initialized) — receives the token being bought
    /// - Current Tick Account, the pool's TWAP PDA derived as
    ///   `compute_current_tick_account_pda(twap_oracle_program_id, pool.account_id)`; refreshed
    ///   with the new spot price
    /// - Clock Account (the canonical 1-block LEZ clock)
    SwapExactOutput {
        exact_amount_out: u128,
        max_amount_in: u128,
        /// Unix timestamp (milliseconds) after which this transaction is invalid.
        deadline: u64,
    },

    /// Sync pool reserves with current vault balances, refreshing the pool's TWAP current tick.
    ///
    /// Required accounts:
    /// - AMM Pool (initialized, with LP supply at or above minimum liquidity)
    /// - Vault Holding Account for Token A (initialized)
    /// - Vault Holding Account for Token B (initialized)
    /// - Current Tick Account, the pool's TWAP PDA derived as
    ///   `compute_current_tick_account_pda(twap_oracle_program_id, pool.account_id)`; refreshed
    ///   with the new spot price
    /// - Clock Account (the canonical 1-block LEZ clock)
    SyncReserves,
}

pub const MINIMUM_LIQUIDITY: u128 = 1_000;

#[account_type]
#[derive(Clone, Default, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct PoolDefinition {
    pub definition_token_a_id: AccountId,
    pub definition_token_b_id: AccountId,
    pub vault_a_id: AccountId,
    pub vault_b_id: AccountId,
    pub liquidity_pool_id: AccountId,
    /// Total LP supply tracked by the pool. After initialization it includes the permanently
    /// locked `MINIMUM_LIQUIDITY`; a zero supply means the pool is uninitialized
    pub liquidity_pool_supply: u128,
    pub reserve_a: u128,
    pub reserve_b: u128,
    /// Fee tier in basis points.
    pub fees: u128,
}

pub const FEE_BPS_DENOMINATOR: u128 = 10_000;
pub const FEE_TIER_BPS_1: u128 = 1;
pub const FEE_TIER_BPS_5: u128 = 5;
pub const FEE_TIER_BPS_30: u128 = 30;
pub const FEE_TIER_BPS_100: u128 = 100;

/// The supported fee tiers (raw bps), ascending — the canonical list for off-chain callers
/// (the module/UI) to enumerate without hardcoding. The guest's check stays `is_supported_fee_tier`
/// below (its `match` is unchanged, so this addition does not affect the program ImageID — the
/// const is unused on-chain); `fee_tiers_list_matches_the_check` locks the two together.
pub const SUPPORTED_FEE_TIERS: [u128; 4] = [
    FEE_TIER_BPS_1,
    FEE_TIER_BPS_5,
    FEE_TIER_BPS_30,
    FEE_TIER_BPS_100,
];

pub fn is_supported_fee_tier(fees: u128) -> bool {
    matches!(
        fees,
        FEE_TIER_BPS_1 | FEE_TIER_BPS_5 | FEE_TIER_BPS_30 | FEE_TIER_BPS_100
    )
}

pub fn assert_supported_fee_tier(fees: u128) {
    assert!(
        is_supported_fee_tier(fees),
        "Fee tier must be one of 1, 5, 30, or 100 basis points"
    );
}

/// Computes a `Q64.64` spot price (`reserve_quote` per `reserve_base`) from raw pool reserves.
///
/// This is the constant-product AMM's spot price (`reserve_quote / reserve_base`) expressed as a
/// `Q64.64` fixed-point value: `(reserve_quote / reserve_base) * 2^64`. It is computed in 256-bit
/// precision and saturates at `u128::MAX` if the ratio exceeds the representable range. The TWAP
/// oracle consumes exactly this representation (it converts the `Q64.64` price to a tick), so the
/// AMM owns the reserves → price mapping and the oracle stays agnostic to how the price is formed.
///
/// # Panics
/// Panics if `reserve_base` is zero.
#[must_use]
pub fn spot_price_q64_64(reserve_base: u128, reserve_quote: u128) -> u128 {
    use alloy_primitives::U256;

    assert!(
        reserve_base != 0,
        "spot_price_q64_64: reserve_base must be non-zero"
    );

    let numerator = U256::from(reserve_quote)
        .checked_shl(64)
        .expect("reserve_quote < 2^128, so reserve_quote << 64 fits in U256");
    let price = numerator
        .checked_div(U256::from(reserve_base))
        .expect("reserve_base is non-zero after the assertion above");

    u128::try_from(price).unwrap_or(u128::MAX)
}

/// `floor(a * b / c)` computed in U256 so the `a * b` product can't overflow u128.
/// (Storage stays u128; only the intermediate widens.)
///
/// # Panics
/// Panics if `c` is zero, or if the result exceeds u128.
#[must_use]
pub fn mul_div_floor(a: u128, b: u128, c: u128) -> u128 {
    use alloy_primitives::U256;
    assert!(c != 0, "mul_div_floor: divisor must be non-zero");
    let product = U256::from(a)
        .checked_mul(U256::from(b))
        .expect("u128 * u128 always fits in U256");
    let result = product
        .checked_div(U256::from(c))
        .expect("mul_div_floor: divisor is non-zero after the assertion above");
    u128::try_from(result).expect("mul_div_floor result exceeds u128")
}

/// `ceil(a * b / c)` computed in U256 so the `a * b` product can't overflow u128.
/// (Storage stays u128; only the intermediate widens.)
///
/// # Panics
/// Panics if `c` is zero, or if the result exceeds u128.
#[must_use]
pub fn mul_div_ceil(a: u128, b: u128, c: u128) -> u128 {
    use alloy_primitives::U256;
    assert!(c != 0, "mul_div_ceil: divisor must be non-zero");
    let product = U256::from(a)
        .checked_mul(U256::from(b))
        .expect("u128 * u128 always fits in U256");
    let result = product.div_ceil(U256::from(c));
    u128::try_from(result).expect("mul_div_ceil result exceeds u128")
}

/// Adverse price impact of a swap in basis points: how far `amount_out` falls
/// below the naive spot-price valuation of `amount_in`
/// (`reserve_out * amount_in / reserve_in`). Display-only — it never panics.
///
/// The naive valuation can exceed u128 for extreme reserve ratios (e.g.
/// `reserve_out` near `u128::MAX` with a tiny `reserve_in`), which is why it is
/// not materialized as u128: this multiplies the *bounded*
/// `FEE_BPS_DENOMINATOR * amount_out` (≤ ~2^142) first and divides by the wide
/// naive value, so every intermediate stays inside U256 and the result is
/// clamped to `[0, FEE_BPS_DENOMINATOR]`. Returns `0` when the naive valuation
/// rounds to zero (or `reserve_in` is zero).
#[must_use]
pub fn price_impact_bps(
    amount_in: u128,
    amount_out: u128,
    reserve_in: u128,
    reserve_out: u128,
) -> u32 {
    use alloy_primitives::U256;
    if reserve_in == 0 {
        return 0;
    }
    let spot = U256::from(reserve_out)
        .checked_mul(U256::from(amount_in))
        .expect("u128 * u128 always fits in U256")
        .checked_div(U256::from(reserve_in))
        .expect("reserve_in is non-zero after the guard above");
    if spot.is_zero() {
        return 0;
    }
    // Fraction of the spot value the trader keeps, in bps (≤ FEE_BPS_DENOMINATOR
    // since amount_out ≤ spot). The numerator is bounded, so no overflow even when
    // `spot` is enormous.
    let kept = U256::from(FEE_BPS_DENOMINATOR)
        .checked_mul(U256::from(amount_out))
        .expect("FEE_BPS_DENOMINATOR * u128 always fits in U256")
        .checked_div(spot)
        .expect("spot is non-zero after the is_zero check above");
    let kept = u128::try_from(kept)
        .unwrap_or(FEE_BPS_DENOMINATOR)
        .min(FEE_BPS_DENOMINATOR);
    u32::try_from(FEE_BPS_DENOMINATOR.saturating_sub(kept)).unwrap_or(u32::MAX)
}

/// The constant-product output for a `SwapExactInput`, matching the AMM's on-chain
/// pricing exactly — used by both `amm_program::swap` and the off-chain swap quote,
/// so the preview and the executed trade agree. Fee-adjusts the input, then applies
/// `reserve_out * effective / (reserve_in + effective)`.
///
/// Returns `(effective_amount_in, amount_out)`; both are `0` when the fee-adjusted
/// input rounds to zero. Saturating: an out-of-range fee, or the impossible
/// `reserve_in + effective` overflow (would need a reserve near `u128::MAX`),
/// degrades to `0` output rather than panicking. Callers validate the fee tier and
/// enforce their own nonzero / min-out checks.
#[must_use]
pub fn swap_exact_in_amounts(
    amount_in: u128,
    reserve_in: u128,
    reserve_out: u128,
    fee_bps: u128,
) -> (u128, u128) {
    let fee_multiplier = FEE_BPS_DENOMINATOR.saturating_sub(fee_bps);
    let effective_amount_in = mul_div_floor(amount_in, fee_multiplier, FEE_BPS_DENOMINATOR);
    let reserve_plus_effective = match reserve_in.checked_add(effective_amount_in) {
        Some(v) => v,
        None => return (effective_amount_in, 0),
    };
    let amount_out = if reserve_plus_effective == 0 {
        0
    } else {
        mul_div_floor(reserve_out, effective_amount_in, reserve_plus_effective)
    };
    (effective_amount_in, amount_out)
}

/// The input amounts for a `SwapExactOutput`: the fee-adjusted (effective) input
/// and the gross input required to receive exactly `amount_out`, matching the AMM's
/// on-chain pricing — used by both `amm_program::swap` and the off-chain
/// exact-output quote. Solves the constant product for the input, then lifts it
/// back through the fee (both steps round up, so the pool never comes up short):
/// `required_in = ceil(ceil(reserve_in * amount_out / (reserve_out - amount_out))
/// * FEE_DENOM / (FEE_DENOM - fee_bps))`.
///
/// Returns `None` when the trade is unfulfillable: `amount_out >= reserve_out` (you
/// can't withdraw the whole pool), `reserve_in == 0` (an empty input reserve has no
/// solution for a positive output — without this it would round to a free
/// `(0, 0)`), or a degenerate `fee_bps >= FEE_DENOM`. Callers enforce their own
/// nonzero / max-in checks.
#[must_use]
pub fn swap_exact_out_amounts(
    amount_out: u128,
    reserve_in: u128,
    reserve_out: u128,
    fee_bps: u128,
) -> Option<(u128, u128)> {
    let denominator = reserve_out.checked_sub(amount_out)?;
    if denominator == 0 {
        return None; // amount_out == reserve_out: draining the pool is impossible
    }
    if reserve_in == 0 {
        return None; // no input reserve: the inverse curve has no solution for a positive output
    }
    let effective_in_min = mul_div_ceil(reserve_in, amount_out, denominator);
    let fee_multiplier = FEE_BPS_DENOMINATOR.checked_sub(fee_bps)?;
    if fee_multiplier == 0 {
        return None; // fee_bps == FEE_DENOM (impossible for a supported tier)
    }
    let required_in = mul_div_ceil(effective_in_min, FEE_BPS_DENOMINATOR, fee_multiplier);
    Some((effective_in_min, required_in))
}

/// `floor(sqrt(a * b))` computed in U256 so the `a * b` product can't overflow u128.
///
/// # Panics
/// Panics if the result exceeds u128.
#[must_use]
pub fn isqrt_product(a: u128, b: u128) -> u128 {
    use alloy_primitives::U256;
    let product = U256::from(a)
        .checked_mul(U256::from(b))
        .expect("u128 * u128 always fits in U256");
    let root = product.root(2); // ruint integer root; floor sqrt
    u128::try_from(root).expect("isqrt_product result exceeds u128")
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

/// Singleton configuration account for the AMM Program.
///
/// Stored at the PDA derived from the constant `"CONFIG"` seed
/// (`compute_config_pda(amm_program_id)`). Created once via the `Initialize` instruction; its
/// existence is the Program's "initialized" flag. Every chained-call instruction reads
/// `token_program_id` from here instead of trusting the program owner of a caller-supplied
/// account.
#[account_type]
#[derive(Clone, Default, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct AmmConfig {
    /// Program ID of the Token Program the AMM issues chained calls to.
    pub token_program_id: ProgramId,
    /// Program ID of the TWAP oracle program the AMM issues chained calls to.
    pub twap_oracle_program_id: ProgramId,
    /// Admin authority allowed to transfer admin control via `UpdateConfig`.
    pub authority: AccountId,
}

impl TryFrom<&Data> for AmmConfig {
    type Error = std::io::Error;

    fn try_from(data: &Data) -> Result<Self, Self::Error> {
        AmmConfig::try_from_slice(data.as_ref())
    }
}

impl From<&AmmConfig> for Data {
    fn from(config: &AmmConfig) -> Self {
        let mut data = Vec::with_capacity(std::mem::size_of_val(config));

        BorshSerialize::serialize(config, &mut data).expect("Serialization to Vec should not fail");

        Data::try_from(data).expect("AMM config encoded data should fit into Data")
    }
}

// Stable seed marker for the singleton config PDA. The literal `"CONFIG"` bytes are hashed into
// the 32-byte seed; this must stay unchanged for address compatibility.
const CONFIG_PDA_SEED: &[u8] = b"CONFIG";

/// Derives the [`AccountId`] of the AMM Program's singleton config PDA.
#[must_use]
pub fn compute_config_pda(amm_program_id: ProgramId) -> AccountId {
    AccountId::for_public_pda(&amm_program_id, &compute_config_pda_seed())
}

/// Derives the [`PdaSeed`] of the AMM Program's singleton config PDA from the `"CONFIG"` bytes.
#[must_use]
pub fn compute_config_pda_seed() -> PdaSeed {
    use risc0_zkvm::sha::{Impl, Sha256};

    PdaSeed::new(
        Impl::hash_bytes(CONFIG_PDA_SEED)
            .as_bytes()
            .try_into()
            .expect("Hash output must be exactly 32 bytes long"),
    )
}

pub fn compute_pool_pda(
    amm_program_id: ProgramId,
    definition_token_a_id: AccountId,
    definition_token_b_id: AccountId,
) -> AccountId {
    AccountId::for_public_pda(
        &amm_program_id,
        &compute_pool_pda_seed(definition_token_a_id, definition_token_b_id),
    )
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
    let (token_1_bytes, token_2_bytes) = bytes.split_at_mut(32);
    token_1_bytes.copy_from_slice(&token_1.to_bytes());
    token_2_bytes.copy_from_slice(&token_2.to_bytes());

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
    AccountId::for_public_pda(
        &amm_program_id,
        &compute_vault_pda_seed(pool_id, definition_token_id),
    )
}

pub fn compute_vault_pda_seed(pool_id: AccountId, definition_token_id: AccountId) -> PdaSeed {
    use risc0_zkvm::sha::{Impl, Sha256};

    let mut bytes = [0; 64];
    let (pool_bytes, definition_bytes) = bytes.split_at_mut(32);
    pool_bytes.copy_from_slice(&pool_id.to_bytes());
    definition_bytes.copy_from_slice(&definition_token_id.to_bytes());

    PdaSeed::new(
        Impl::hash_bytes(&bytes)
            .as_bytes()
            .try_into()
            .expect("Hash output must be exactly 32 bytes long"),
    )
}

pub fn compute_liquidity_token_pda(amm_program_id: ProgramId, pool_id: AccountId) -> AccountId {
    AccountId::for_public_pda(&amm_program_id, &compute_liquidity_token_pda_seed(pool_id))
}

pub fn compute_liquidity_token_pda_seed(pool_id: AccountId) -> PdaSeed {
    use risc0_zkvm::sha::{Impl, Sha256};

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&pool_id.to_bytes());
    bytes.extend_from_slice(LIQUIDITY_TOKEN_PDA_SEED);

    PdaSeed::new(
        Impl::hash_bytes(&bytes)
            .as_bytes()
            .try_into()
            .expect("Hash output must be exactly 32 bytes long"),
    )
}

pub fn compute_lp_lock_holding_pda(amm_program_id: ProgramId, pool_id: AccountId) -> AccountId {
    AccountId::for_public_pda(&amm_program_id, &compute_lp_lock_holding_pda_seed(pool_id))
}

pub fn compute_lp_lock_holding_pda_seed(pool_id: AccountId) -> PdaSeed {
    use risc0_zkvm::sha::{Impl, Sha256};

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&pool_id.to_bytes());
    bytes.extend_from_slice(LP_LOCK_HOLDING_PDA_SEED);

    PdaSeed::new(
        Impl::hash_bytes(&bytes)
            .as_bytes()
            .try_into()
            .expect("Hash output must be exactly 32 bytes long"),
    )
}

fn read_fungible_holding(account: &AccountWithMetadata, context: &str) -> (AccountId, u128) {
    let token_holding = token_core::TokenHolding::try_from(&account.account.data)
        .unwrap_or_else(|_| panic!("{context}: AMM Program expects a valid Token Holding Account"));

    let token_core::TokenHolding::Fungible {
        definition_id,
        balance,
    } = token_holding
    else {
        panic!("{context}: AMM Program expects a valid Fungible Token Holding Account");
    };

    (definition_id, balance)
}

pub fn read_vault_fungible_balances(
    context: &str,
    vault_a: &AccountWithMetadata,
    vault_b: &AccountWithMetadata,
) -> (u128, u128) {
    let vault_a_context = format!("{context}: Vault A");
    let vault_b_context = format!("{context}: Vault B");
    let (_, vault_a_balance) = read_fungible_holding(vault_a, &vault_a_context);
    let (_, vault_b_balance) = read_fungible_holding(vault_b, &vault_b_context);

    (vault_a_balance, vault_b_balance)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `1.0` in Q64.64 is `2^64`.
    const ONE_Q64_64: u128 = 1u128 << 64;

    #[test]
    fn fee_tiers_list_matches_the_check() {
        // The enumerated list must agree with the guest's `is_supported_fee_tier` match: every
        // listed tier is accepted, and nothing adjacent to them is (guards a drifted list).
        for tier in SUPPORTED_FEE_TIERS {
            assert!(
                is_supported_fee_tier(tier),
                "{tier} listed but not accepted"
            );
        }
        for probe in [0u128, 2, 4, 6, 29, 31, 99, 101, 10_000] {
            assert!(
                !is_supported_fee_tier(probe),
                "{probe} accepted but not listed"
            );
        }
        // Ascending and de-duplicated.
        assert!(SUPPORTED_FEE_TIERS.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn equal_reserves_map_to_unit_price() {
        assert_eq!(spot_price_q64_64(1_000, 1_000), ONE_Q64_64);
    }

    #[test]
    fn spot_price_reflects_reserve_ratio() {
        // reserve_quote / reserve_base = 2.0 -> 2 * 2^64.
        assert_eq!(spot_price_q64_64(1_000, 2_000), ONE_Q64_64 * 2);
        // reserve_quote / reserve_base = 0.5 -> 2^64 / 2.
        assert_eq!(spot_price_q64_64(2_000, 1_000), ONE_Q64_64 / 2);
    }

    #[test]
    fn spot_price_saturates_instead_of_overflowing() {
        // A huge quote-to-base ratio would exceed u128 in Q64.64; it must saturate, not panic.
        assert_eq!(spot_price_q64_64(1, u128::MAX), u128::MAX);
    }

    #[test]
    fn spot_price_handles_large_reserves_without_intermediate_overflow() {
        // reserve_quote >= 2^64 would overflow a naive `reserve_quote << 64` in u128; the U256
        // intermediate keeps it exact. Ratio here is 4.0.
        let base = 1u128 << 64;
        let quote = 1u128 << 66;
        assert_eq!(spot_price_q64_64(base, quote), ONE_Q64_64 * 4);
    }

    #[test]
    #[should_panic(expected = "reserve_base must be non-zero")]
    fn zero_reserve_base_panics() {
        let _ = spot_price_q64_64(0, 1_000);
    }

    #[test]
    fn mul_div_floor_small_cases() {
        assert_eq!(mul_div_floor(6, 7, 3), 14);
        // floor(7 * 7 / 3) = floor(49/3) = 16
        assert_eq!(mul_div_floor(7, 7, 3), 16);
        assert_eq!(mul_div_floor(0, 12345, 7), 0);
        assert_eq!(mul_div_floor(1, 1, 2), 0);
    }

    #[test]
    fn mul_div_floor_product_exceeds_u128() {
        // 2e30 * 2e30 = 4e60, far beyond u128; / 1e20 = 4e40, still beyond u128 -- but the
        // intermediate must not overflow and the *quotient* here fits once divided down.
        // 2e30 * 2e30 / 2e30 = 2e30 fits in u128.
        let a = 2_000_000_000_000_000_000_000_000_000_000u128; // 2e30
        assert_eq!(mul_div_floor(a, a, a), a);
        // 2e30 * 2e30 / 1e20 = 4e40 would exceed u128 -- verify it panics on downcast.
    }

    #[test]
    #[should_panic(expected = "mul_div_floor result exceeds u128")]
    fn mul_div_floor_result_exceeds_u128_panics() {
        let a = 2_000_000_000_000_000_000_000_000_000_000u128; // 2e30
        let c = 100_000_000_000_000_000_000u128; // 1e20
        let _ = mul_div_floor(a, a, c); // 4e40 > u128::MAX
    }

    #[test]
    #[should_panic(expected = "mul_div_floor: divisor must be non-zero")]
    fn mul_div_floor_zero_divisor_panics() {
        let _ = mul_div_floor(1, 2, 0);
    }

    #[test]
    fn swap_exact_in_amounts_matches_constant_product() {
        // 0.30% fee, reserves 1_000_000 in / 2_000_000 out, amount_in 10_000.
        let (eff, out) = swap_exact_in_amounts(10_000, 1_000_000, 2_000_000, 30);
        let expected_eff = 10_000 * (10_000 - 30) / 10_000;
        let expected_out = 2_000_000 * expected_eff / (1_000_000 + expected_eff);
        assert_eq!((eff, out), (expected_eff, expected_out));

        // A tiny input can fee-round the effective input to zero → zero output.
        assert_eq!(swap_exact_in_amounts(1, 1_000_000, 2_000_000, 30), (0, 0));

        // Degenerate reserves saturate rather than dividing by zero.
        assert_eq!(swap_exact_in_amounts(1, 0, 0, 30), (0, 0));
    }

    #[test]
    fn price_impact_bps_is_bounded_and_never_panics() {
        let max_bps = u32::try_from(FEE_BPS_DENOMINATOR).unwrap();

        // Normal pool: a small trade has a bounded impact.
        let (_, out) = swap_exact_in_amounts(10_000, 1_000_000, 2_000_000, 30);
        assert!(price_impact_bps(10_000, out, 1_000_000, 2_000_000) <= max_bps);

        // Extreme reserve ratio: the naive spot valuation
        // (reserve_out * amount_in / reserve_in) exceeds u128 — a u128 mul_div here
        // would panic. This must stay bounded and not panic.
        let reserve_out = u128::MAX;
        let (_, out) = swap_exact_in_amounts(2, 1, reserve_out, 30);
        assert!(price_impact_bps(2, out, 1, reserve_out) <= max_bps);

        // Zero naive value / zero reserve → zero impact, no division by zero.
        assert_eq!(price_impact_bps(1, 0, 0, 0), 0);
        assert_eq!(price_impact_bps(1, 0, 1_000_000, 0), 0);
    }

    #[test]
    fn swap_exact_out_amounts_inverts_the_constant_product() {
        // 0.30% fee, reserves 1_000_000 in / 2_000_000 out, want 10_000 out.
        let (eff_min, required_in) =
            swap_exact_out_amounts(10_000, 1_000_000, 2_000_000, 30).unwrap();
        // effective_in >= ceil(reserve_in * out / (reserve_out - out)); then lift through fee.
        let expected_eff = (1_000_000u128 * 10_000).div_ceil(2_000_000 - 10_000);
        let expected_in = (expected_eff * 10_000).div_ceil(10_000 - 30);
        assert_eq!((eff_min, required_in), (expected_eff, expected_in));

        // Round-trips with the forward quote: paying required_in yields at least the ask.
        let (_, out) = swap_exact_in_amounts(required_in, 1_000_000, 2_000_000, 30);
        assert!(out >= 10_000);

        // Unfulfillable: can't withdraw the whole pool (or more).
        assert_eq!(
            swap_exact_out_amounts(2_000_000, 1_000_000, 2_000_000, 30),
            None
        );
        assert_eq!(
            swap_exact_out_amounts(2_000_001, 1_000_000, 2_000_000, 30),
            None
        );
        // No input reserve → no valid input for a positive output (not a free (0, 0)).
        assert_eq!(swap_exact_out_amounts(10_000, 0, 2_000_000, 30), None);
    }

    #[test]
    fn mul_div_ceil_small_cases() {
        assert_eq!(mul_div_ceil(6, 7, 3), 14);
        // ceil(7 * 7 / 3) = ceil(49/3) = 17
        assert_eq!(mul_div_ceil(7, 7, 3), 17);
        // exact division: no rounding up
        assert_eq!(mul_div_ceil(6, 4, 3), 8);
        assert_eq!(mul_div_ceil(0, 12345, 7), 0);
    }

    #[test]
    fn mul_div_ceil_product_exceeds_u128() {
        // (2e30 * 2e30) / 2e30 = 2e30 exactly, fits in u128.
        let a = 2_000_000_000_000_000_000_000_000_000_000u128; // 2e30
        assert_eq!(mul_div_ceil(a, a, a), a);
    }

    #[test]
    #[should_panic(expected = "mul_div_ceil: divisor must be non-zero")]
    fn mul_div_ceil_zero_divisor_panics() {
        let _ = mul_div_ceil(1, 2, 0);
    }

    #[test]
    fn isqrt_product_matches_u128_isqrt_for_small_values() {
        assert_eq!(isqrt_product(100, 100), 100);
        assert_eq!(isqrt_product(2, 8), 4);
        // floor(sqrt(7 * 7)) = 7, floor(sqrt(50)) = 7
        assert_eq!(isqrt_product(7, 7), 7);
        assert_eq!(isqrt_product(5, 10), 50u128.isqrt());
    }

    #[test]
    fn isqrt_product_handles_the_1e20_times_2e20_overflow_case() {
        // 1e20 * 2e20 = 2e40 overflows u128 (max ~3.4e38); the U256 intermediate keeps it exact.
        let a = 100_000_000_000_000_000_000u128; // 1e20
        let b = 200_000_000_000_000_000_000u128; // 2e20
                                                 // floor(sqrt(2e40)) computed independently in U256.
        let expected = {
            use alloy_primitives::U256;
            let product = U256::from(a).checked_mul(U256::from(b)).unwrap();
            u128::try_from(product.root(2)).unwrap()
        };
        assert_eq!(isqrt_product(a, b), expected);
        // Sanity: floor(sqrt(2e40)) = floor(1.4142...e20) = 141421356237309504880.
        assert_eq!(isqrt_product(a, b), 141_421_356_237_309_504_880);
    }
}
