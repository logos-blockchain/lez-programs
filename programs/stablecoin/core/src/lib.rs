//! Core data structures and utilities for the Stablecoin Program.

pub mod math;

use borsh::{BorshDeserialize, BorshSerialize};
use nssa_core::{
    account::{AccountId, AccountWithMetadata, Data},
    program::{PdaSeed, ProgramId},
};
use serde::{Deserialize, Serialize};
use spel_framework_macros::account_type;

// Stable domain-separation tags for the position PDAs; these must stay unchanged for address
// compatibility.
const POSITION_PDA_DOMAIN: &[u8] = b"POSITION";
const POSITION_VAULT_PDA_DOMAIN: &[u8] = b"POSITION_VAULT";
const REDEMPTION_CONTROLLER_PDA_DOMAIN: &[u8] = b"REDEMPTION_CONTROLLER";

/// Fixed-point denominator for controller gain parameters.
///
/// A gain of [`CONTROLLER_GAIN_SCALE`] means `1.0`.
pub const CONTROLLER_GAIN_SCALE: u128 = 1_000_000_000;

/// Stablecoin Program Instruction.
#[derive(Debug, Serialize, Deserialize)]
pub enum Instruction {
    /// Open a new collateral-only [`Position`] for the calling owner.
    ///
    /// Required accounts (5):
    /// - Owner account (authorized)
    /// - Position account (uninitialized, address must match
    ///   `compute_position_pda(self_program_id, owner, token_definition)`)
    /// - Position vault token holding account (uninitialized, address must match
    ///   `compute_position_vault_pda(self_program_id, position_id)`)
    /// - Owner's source token holding for the collateral (authorized, initialized)
    /// - Token definition account for the collateral (matches the user holding's `definition_id`;
    ///   its `program_owner` determines the Token Program used by the chained `InitializeAccount`
    ///   / `Transfer` calls)
    OpenPosition {
        /// Amount of collateral tokens to deposit into the position vault.
        collateral_amount: u128,
    },
    /// Withdraw `amount` collateral tokens from a position back to a user-controlled holding.
    ///
    /// Required accounts (4):
    /// - Owner account (authorized)
    /// - Position account (initialized, owned by `self_program_id`)
    /// - Position vault token holding (address must match
    ///   `compute_position_vault_pda(self_program_id, position_id)`)
    /// - Destination user collateral holding (initialized, owned by the vault's Token Program,
    ///   `TokenHolding.definition_id == Position.collateral_definition_id`)
    ///
    /// `token_program_id` is derived from `vault.account.program_owner`;
    /// `collateral_definition_id` is read from the decoded [`Position`].
    ///
    /// **Note:** until issues #97/#96/#95 land, this instruction hard-asserts
    /// `Position.debt_amount == 0` instead of accruing fees and checking the
    /// collateralization ratio.
    WithdrawCollateral {
        /// Amount of collateral tokens to move from the vault back to `destination`.
        amount: u128,
    },
    /// Repay `amount` of outstanding stablecoin debt against an existing position.
    ///
    /// Required accounts (4):
    /// - Owner account (authorized; binds caller-as-owner via position PDA re-derivation)
    /// - Position account (initialized, owned by `self_program_id`)
    /// - Stablecoin token definition account (the definition of the stablecoin being repaid)
    /// - User's stablecoin holding (authorized, initialized, owned by the same Token Program as
    ///   the definition, with `TokenHolding.definition_id == stablecoin_definition.account_id`)
    ///
    /// `token_program_id` is derived from `user_stablecoin_holding.account.program_owner`.
    /// `collateral_definition_id` (for position PDA verification) is read from the
    /// decoded [`Position`].
    ///
    /// **Note:** until issue #97 (stability fee accrual) lands, this instruction does
    /// not accrue fees before reducing debt. A `// TODO(#97)` comment in the host
    /// function marks where the accrual code will plug in. Today every position has
    /// `debt_amount = 0` (no `generate_debt` yet), so the precondition is vacuously met.
    ///
    /// **Note:** until issue #91 (`generate_debt`) records the stablecoin definition
    /// into `Position`, this instruction cannot validate that the passed
    /// `stablecoin_token_definition` is the one this position's debt is denominated
    /// in. The caller is trusted for that until then.
    RepayDebt {
        /// Amount of stablecoin debt to repay (also the amount burned from the user's holding).
        amount: u128,
    },
    /// Initialize the global redemption-rate feedback controller for one stablecoin/feed pair.
    ///
    /// Required accounts (3):
    /// - Redemption controller account (uninitialized, address must match
    ///   `compute_redemption_controller_pda(self_program_id, stablecoin_definition, price_feed)`)
    /// - Stablecoin token definition account (initialized fungible token)
    /// - Oracle price feed account (initialized; its `program_owner` becomes the configured oracle
    ///   program)
    ///
    /// `proportional_gain` and `integral_gain` use [`CONTROLLER_GAIN_SCALE`] fixed-point
    /// precision. For example, `CONTROLLER_GAIN_SCALE / 10` represents `0.1`.
    InitializeRedemptionController {
        /// Asset that denominates both the oracle market price and redemption price.
        reference_asset_id: AccountId,
        /// Initial redemption price, in the same units and precision as the oracle price.
        initial_redemption_price: u128,
        /// Proportional controller gain, scaled by [`CONTROLLER_GAIN_SCALE`].
        proportional_gain: u128,
        /// Integral controller gain, scaled by [`CONTROLLER_GAIN_SCALE`].
        integral_gain: u128,
        /// Maximum absolute accumulated error before the integral term is clamped.
        max_integral_error: u128,
        /// Maximum absolute redemption rate, in price units per timestamp unit.
        max_redemption_rate: u128,
        /// Maximum allowed oracle price age. Uses the same timestamp unit as the oracle feed.
        max_price_feed_age: u64,
        /// Timestamp used to initialize the controller state.
        current_timestamp: u64,
    },
    /// Permissionlessly update redemption price and rate from the configured price feed.
    ///
    /// Required accounts (2):
    /// - Redemption controller account (initialized, owned by `self_program_id`)
    /// - Configured oracle price feed account
    ///
    /// If the configured feed is stale or unavailable, the controller account is emitted
    /// unchanged so redemption updates are paused.
    UpdateRedemptionController {
        /// Current block timestamp. The guest constrains output validity to this exact timestamp.
        current_timestamp: u64,
    },
}

/// Persistent state held by a Stablecoin [`Position`] account.
///
/// `debt_amount` is included for forward compatibility with `generate_debt`; until that
/// instruction lands `open_position` always initializes it to `0`.
#[account_type]
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct Position {
    /// Token holding account (vault PDA) that custodies the collateral backing this position.
    pub collateral_vault_id: AccountId,
    /// Token definition for the collateral held in `collateral_vault_id`.
    pub collateral_definition_id: AccountId,
    /// Amount of collateral tokens deposited.
    pub collateral_amount: u128,
    /// Outstanding stablecoin debt against this position.
    pub debt_amount: u128,
}

/// Global redemption feedback controller state for a stablecoin/feed pair.
///
/// `redemption_rate` is signed price drift per timestamp unit. Positive rates raise the
/// redemption price; negative rates lower it. `accumulated_error` stores the integral term
/// before gain scaling and is clamped by `max_integral_error`.
#[account_type]
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct RedemptionController {
    /// Stablecoin token definition priced by the oracle feed.
    pub stablecoin_definition_id: AccountId,
    /// Asset that denominates both oracle prices and redemption price.
    pub reference_asset_id: AccountId,
    /// Configured oracle price feed account.
    pub price_feed_id: AccountId,
    /// Program expected to own `price_feed_id`.
    pub oracle_program_id: ProgramId,
    /// Current redemption price in oracle price units.
    pub redemption_price: u128,
    /// Current redemption rate in price units per timestamp unit.
    pub redemption_rate: i128,
    /// Integral controller state, clamped to `max_integral_error`.
    pub accumulated_error: i128,
    /// Proportional controller gain, scaled by [`CONTROLLER_GAIN_SCALE`].
    pub proportional_gain: u128,
    /// Integral controller gain, scaled by [`CONTROLLER_GAIN_SCALE`].
    pub integral_gain: u128,
    /// Maximum absolute accumulated error.
    pub max_integral_error: u128,
    /// Maximum absolute redemption rate.
    pub max_redemption_rate: u128,
    /// Maximum allowed oracle price age.
    pub max_price_feed_age: u64,
    /// Last timestamp at which the controller accepted a live oracle reading.
    pub last_update_timestamp: u64,
}

impl TryFrom<&Data> for Position {
    type Error = std::io::Error;

    fn try_from(data: &Data) -> Result<Self, Self::Error> {
        Self::try_from_slice(data.as_ref())
    }
}

impl From<&Position> for Data {
    fn from(position: &Position) -> Self {
        let mut data = Vec::with_capacity(std::mem::size_of_val(position));
        BorshSerialize::serialize(position, &mut data)
            .expect("Serialization to Vec should not fail");
        Self::try_from(data).expect("Position encoded data should fit into Data")
    }
}

impl TryFrom<&Data> for RedemptionController {
    type Error = std::io::Error;

    fn try_from(data: &Data) -> Result<Self, Self::Error> {
        Self::try_from_slice(data.as_ref())
    }
}

impl From<&RedemptionController> for Data {
    fn from(controller: &RedemptionController) -> Self {
        let mut data = Vec::with_capacity(std::mem::size_of_val(controller));
        BorshSerialize::serialize(controller, &mut data)
            .expect("Serialization to Vec should not fail");
        Self::try_from(data).expect("Redemption controller encoded data should fit into Data")
    }
}

/// PDA seed for the [`Position`] account owned by `owner_id` for `collateral_definition_id`.
///
/// Derived from the owner and collateral definition addresses with a domain-separation tag
/// so one owner can hold separate positions for separate collateral definitions.
pub fn compute_position_pda_seed(
    owner_id: AccountId,
    collateral_definition_id: AccountId,
) -> PdaSeed {
    use risc0_zkvm::sha::{Impl, Sha256 as _};

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&owner_id.to_bytes());
    bytes.extend_from_slice(&collateral_definition_id.to_bytes());
    bytes.extend_from_slice(POSITION_PDA_DOMAIN);

    let mut out = [0u8; 32];
    out.copy_from_slice(Impl::hash_bytes(&bytes).as_bytes());
    PdaSeed::new(out)
}

/// Account id of the [`Position`] PDA owned by `owner_id` under `stablecoin_program_id`.
pub fn compute_position_pda(
    stablecoin_program_id: ProgramId,
    owner_id: AccountId,
    collateral_definition_id: AccountId,
) -> AccountId {
    AccountId::for_public_pda(
        &stablecoin_program_id,
        &compute_position_pda_seed(owner_id, collateral_definition_id),
    )
}

/// PDA seed for the collateral vault token holding bound to a [`Position`].
///
/// Derived from the position's address with a distinct domain-separation tag so the vault
/// id cannot collide with the position id even though both PDAs share the same program.
pub fn compute_position_vault_pda_seed(position_id: AccountId) -> PdaSeed {
    use risc0_zkvm::sha::{Impl, Sha256 as _};

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&position_id.to_bytes());
    bytes.extend_from_slice(POSITION_VAULT_PDA_DOMAIN);

    let mut out = [0u8; 32];
    out.copy_from_slice(Impl::hash_bytes(&bytes).as_bytes());
    PdaSeed::new(out)
}

/// Account id of the collateral vault PDA for `position_id` under `stablecoin_program_id`.
pub fn compute_position_vault_pda(
    stablecoin_program_id: ProgramId,
    position_id: AccountId,
) -> AccountId {
    AccountId::for_public_pda(
        &stablecoin_program_id,
        &compute_position_vault_pda_seed(position_id),
    )
}

/// PDA seed for the [`RedemptionController`] bound to a stablecoin and price feed.
pub fn compute_redemption_controller_pda_seed(
    stablecoin_definition_id: AccountId,
    price_feed_id: AccountId,
) -> PdaSeed {
    use risc0_zkvm::sha::{Impl, Sha256 as _};

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&stablecoin_definition_id.to_bytes());
    bytes.extend_from_slice(&price_feed_id.to_bytes());
    bytes.extend_from_slice(REDEMPTION_CONTROLLER_PDA_DOMAIN);

    let mut out = [0u8; 32];
    out.copy_from_slice(Impl::hash_bytes(&bytes).as_bytes());
    PdaSeed::new(out)
}

/// Account id of the [`RedemptionController`] PDA for a stablecoin/feed pair.
pub fn compute_redemption_controller_pda(
    stablecoin_program_id: ProgramId,
    stablecoin_definition_id: AccountId,
    price_feed_id: AccountId,
) -> AccountId {
    AccountId::for_public_pda(
        &stablecoin_program_id,
        &compute_redemption_controller_pda_seed(stablecoin_definition_id, price_feed_id),
    )
}

/// Verify a redemption controller account address and return its PDA seed.
///
/// # Panics
/// If `controller.account_id` does not match the configured PDA derivation.
pub fn verify_redemption_controller_and_get_seed(
    controller: &AccountWithMetadata,
    stablecoin_definition_id: AccountId,
    price_feed_id: AccountId,
    stablecoin_program_id: ProgramId,
) -> PdaSeed {
    let seed = compute_redemption_controller_pda_seed(stablecoin_definition_id, price_feed_id);
    let expected_id = AccountId::for_public_pda(&stablecoin_program_id, &seed);
    assert_eq!(
        controller.account_id, expected_id,
        "Redemption controller account ID does not match expected derivation"
    );
    seed
}

/// Verify the position account's address matches
/// `(stablecoin_program_id, owner, collateral_definition_id)` and return the [`PdaSeed`] for
/// use in post-state claims.
///
/// # Panics
/// If `position.account_id` does not match the address derived from `owner`,
/// `collateral_definition_id`, and `stablecoin_program_id`.
pub fn verify_position_and_get_seed(
    position: &AccountWithMetadata,
    owner: &AccountWithMetadata,
    collateral_definition_id: AccountId,
    stablecoin_program_id: ProgramId,
) -> PdaSeed {
    let seed = compute_position_pda_seed(owner.account_id, collateral_definition_id);
    let expected_id = AccountId::for_public_pda(&stablecoin_program_id, &seed);
    assert_eq!(
        position.account_id, expected_id,
        "Position account ID does not match expected derivation"
    );
    seed
}

/// Verify the vault account's address matches `(stablecoin_program_id, position)` and
/// return the [`PdaSeed`] for use in chained calls.
///
/// # Panics
/// If `vault.account_id` does not match the address derived from `position_id` and
/// `stablecoin_program_id`.
pub fn verify_position_vault_and_get_seed(
    vault: &AccountWithMetadata,
    position_id: AccountId,
    stablecoin_program_id: ProgramId,
) -> PdaSeed {
    let seed = compute_position_vault_pda_seed(position_id);
    let expected_id = AccountId::for_public_pda(&stablecoin_program_id, &seed);
    assert_eq!(
        vault.account_id, expected_id,
        "Position vault account ID does not match expected derivation"
    );
    seed
}
