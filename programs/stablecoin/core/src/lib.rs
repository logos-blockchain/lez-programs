//! Core data structures and utilities for the Stablecoin Program.

pub mod math;

pub mod protocol_parameters;

pub mod redemption_price_state;

pub mod stability_fee_accumulator;

use borsh::{BorshDeserialize, BorshSerialize};
use nssa_core::{
    account::{AccountId, AccountWithMetadata, Data},
    program::{PdaSeed, ProgramId},
};
pub use protocol_parameters::{
    compute_protocol_parameters_pda, compute_protocol_parameters_pda_seed, ProtocolParameters,
};
pub use redemption_price_state::{
    compute_redemption_price_state_pda, compute_redemption_price_state_pda_seed,
    RedemptionPriceState,
};
use serde::{Deserialize, Serialize};
use spel_framework_macros::account_type;
pub use stability_fee_accumulator::{
    compute_stability_fee_accumulator_pda, compute_stability_fee_accumulator_pda_seed,
    StabilityFeeAccumulator,
};

// Stable domain-separation tags for the position PDAs; these must stay unchanged for address
// compatibility.
const POSITION_PDA_DOMAIN: &[u8] = b"POSITION";
const POSITION_VAULT_PDA_DOMAIN: &[u8] = b"POSITION_VAULT";

/// Stablecoin Program Instruction.
#[derive(Debug, Serialize, Deserialize)]
pub enum Instruction {
    /// Bootstrap the protocol. One-shot — fails if any of the five global PDAs
    /// is already initialized.
    ///
    /// Required accounts (9), in order:
    /// 1. `admin` — authorized; becomes `ProtocolParameters.admin_account_id`.
    /// 2. `protocol_parameters` — uninitialized, PDA at
    ///    `compute_protocol_parameters_pda(self_program_id)`.
    /// 3. `stability_fee_accumulator` — uninitialized, PDA at
    ///    `compute_stability_fee_accumulator_pda(self_program_id)`.
    /// 4. `redemption_price_state` — uninitialized, PDA at
    ///    `compute_redemption_price_state_pda(self_program_id)`.
    /// 5. `stablecoin_definition` — uninitialized, PDA at
    ///    `compute_stablecoin_definition_pda(self_program_id)`; claimed via the chained
    ///    `Token::NewFungibleDefinition`.
    /// 6. `stablecoin_master_holding` — uninitialized, PDA at
    ///    `compute_stablecoin_master_holding_pda(self_program_id)`; also claimed via the chained
    ///    call (Token-Program API artifact — receives `total_supply = 0`).
    /// 7. `collateral_definition` — initialized; validated as `TokenDefinition::Fungible` and
    ///    recorded into `ProtocolParameters.collateral_definition_id` (immutable thereafter).
    /// 8. `market_price_oracle` — initialized; validated as an `OraclePriceAccount` with
    ///    `base_asset` = stablecoin definition, `quote_asset` = collateral definition.
    /// 9. `clock` — the system `CLOCK_01` account; read-only. Its timestamp anchors the
    ///    accumulator and redemption-price state. (The pinned spel-framework exposes no
    ///    `ProgramContext` clock, so wall-clock time is read from this account.)
    InitializeProgram {
        /// Designated freeze authority. Held in `ProtocolParameters`; can be
        /// rotated post-init via `set_freeze_authority`.
        freeze_authority_account_id: AccountId,
        /// Per-millisecond stability fee multiplier in fixed-point. Bound:
        /// `FIXED_POINT_ONE <= x <= FIXED_POINT_ONE * 2` (see spec §8).
        initial_stability_fee_per_millisecond: u128,
        /// PI controller `Kp`. Signed. Bound: `|x| <= FIXED_POINT_ONE * 10^3`.
        initial_controller_proportional_gain: i128,
        /// PI controller `Ki`. Signed. Bound: `|x| <= FIXED_POINT_ONE`.
        initial_controller_integral_gain: i128,
        /// Minimum collateralization ratio in fixed-point. Bound:
        /// `FIXED_POINT_ONE * 1.1 <= x <= FIXED_POINT_ONE * 10`.
        initial_minimum_collateralization_ratio: u128,
        /// Min milliseconds between `update_redemption_rate` calls. Bound: 1..=86_400_000.
        minimum_milliseconds_between_rate_updates: u64,
        /// Max accepted oracle observation age, in milliseconds. Bound: 1..=86_400_000.
        maximum_oracle_price_age_milliseconds: u64,
        /// Initial redemption price in fixed-point (collateral-per-stablecoin).
        /// Must be > 0; chosen by the deployer to reflect the launch target.
        initial_redemption_price: u128,
        /// Name baked into the stablecoin's `TokenDefinition::Fungible`.
        stablecoin_name: String,
    },
    /// Open a new collateral-only [`Position`] for the calling owner.
    ///
    /// Required accounts (5):
    /// - Owner account (authorized)
    /// - Position account (uninitialized, address must match
    ///   `compute_position_pda(self_program_id, owner, position_nonce)`)
    /// - Position vault token holding account (uninitialized, address must match
    ///   `compute_position_vault_pda(self_program_id, position_id)`)
    /// - Owner's source token holding for the collateral (authorized, initialized)
    /// - Token definition account for the collateral (matches the user holding's `definition_id`;
    ///   its `program_owner` determines the Token Program used by the chained `InitializeAccount`
    ///   / `Transfer` calls)
    OpenPosition {
        /// Caller-chosen nonce that, with the owner's account id, forms the
        /// position PDA's seed pre-image. Lets one owner hold many positions.
        position_nonce: u64,
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
    ///   `TokenHolding.definition_id` matches the vault holding's definition)
    ///
    /// `token_program_id` is derived from `vault.account.program_owner`;
    /// the collateral definition is read from the PDA-verified vault holding.
    ///
    /// **Note:** until issues #97/#95 land, this instruction hard-asserts
    /// `Position.normalized_debt_amount == 0` instead of accruing fees and
    /// checking the collateralization ratio.
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
    /// `position_nonce` (for position PDA verification) is read from the
    /// decoded [`Position`].
    ///
    /// **Note:** until issue #97 (stability fee accrual) lands, this instruction does
    /// not accrue fees before reducing debt. A `// TODO(#97)` comment in the host
    /// function marks where the accrual code will plug in. Today every position has
    /// `normalized_debt_amount = 0` (no `generate_debt` yet), so the precondition
    /// is vacuously met.
    ///
    /// **Note:** until issue #91 (`generate_debt`) records the stablecoin definition
    /// into `Position`, this instruction cannot validate that the passed
    /// `stablecoin_token_definition` is the one this position's debt is denominated
    /// in. The caller is trusted for that until then.
    RepayDebt {
        /// Amount of stablecoin debt to repay (also the amount burned from the user's holding).
        amount: u128,
    },
}

/// Persistent state held by a Stablecoin [`Position`] account.
///
/// See spec §4.4. `normalized_debt_amount` is the RAI-style "shares in a debt
/// pool whose value per share is the stability-fee accumulator" — multiply by
/// the current accumulator to get the position's nominal debt at any moment.
#[account_type]
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct Position {
    /// Owner of the position. Must be `is_authorized` for every position op.
    /// Stored for client discovery (PDA seed isn't reversible).
    pub owner_account_id: AccountId,
    /// Caller-chosen nonce; together with `owner_account_id` forms the PDA
    /// seed for this position.
    pub position_nonce: u64,
    /// Collateral vault PDA (= `compute_position_vault_pda(program, position_id)`).
    /// Stored explicitly for op-time efficiency.
    pub vault_account_id: AccountId,
    /// Collateral tokens currently held in the vault. Invariant:
    /// equals `vault_holding.balance` after every modifying op.
    pub collateral_amount: u128,
    /// Stablecoin atomic units divided by the accumulator at mint time.
    /// Nominal debt at time T = `normalized_debt_amount * accumulated_rate(T) / FIXED_POINT_ONE`.
    pub normalized_debt_amount: u128,
    /// Unix milliseconds when the position was first opened. UX/analytics only;
    /// not used in protocol logic.
    pub opened_at: u64,
}

impl TryFrom<&Data> for Position {
    type Error = std::io::Error;

    fn try_from(data: &Data) -> Result<Self, Self::Error> {
        Self::try_from_slice(data.as_ref())
    }
}

impl From<&Position> for Data {
    fn from(position: &Position) -> Self {
        let len = borsh::object_length(position).expect("Position length must be known");
        let mut buf = Vec::with_capacity(len);
        BorshSerialize::serialize(position, &mut buf)
            .expect("Position serialization should not fail");
        Self::try_from(buf).expect("Position encoded data should fit into Data")
    }
}

/// PDA seed for the [`Position`] account at `(owner_id, position_nonce)`.
///
/// The single-instance protocol has only one collateral definition globally
/// (stored on `ProtocolParameters`), so it no longer factors into the seed.
/// The 64-bit `position_nonce` is caller-chosen and lets one owner hold
/// many positions (spec §3.2).
#[must_use]
pub fn compute_position_pda_seed(owner_id: AccountId, position_nonce: u64) -> PdaSeed {
    use risc0_zkvm::sha::{Impl, Sha256 as _};

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&owner_id.to_bytes());
    bytes.extend_from_slice(&position_nonce.to_le_bytes());
    bytes.extend_from_slice(POSITION_PDA_DOMAIN);

    let mut out = [0u8; 32];
    out.copy_from_slice(Impl::hash_bytes(&bytes).as_bytes());
    PdaSeed::new(out)
}

/// Account id of the [`Position`] PDA for `(owner_id, position_nonce)` under
/// `stablecoin_program_id`.
#[must_use]
pub fn compute_position_pda(
    stablecoin_program_id: ProgramId,
    owner_id: AccountId,
    position_nonce: u64,
) -> AccountId {
    AccountId::for_public_pda(
        &stablecoin_program_id,
        &compute_position_pda_seed(owner_id, position_nonce),
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

/// Verify the position account's address matches `(stablecoin_program_id,
/// owner, position_nonce)` and return the [`PdaSeed`] for use in post-state
/// claims.
///
/// # Panics
/// If `position.account_id` does not match the derived PDA.
pub fn verify_position_and_get_seed(
    position: &AccountWithMetadata,
    owner: &AccountWithMetadata,
    position_nonce: u64,
    stablecoin_program_id: ProgramId,
) -> PdaSeed {
    let seed = compute_position_pda_seed(owner.account_id, position_nonce);
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

#[cfg(test)]
mod instruction_tests {
    use super::*;
    use crate::math::FIXED_POINT_ONE;

    #[test]
    fn initialize_program_json_roundtrip() {
        let original = Instruction::InitializeProgram {
            freeze_authority_account_id: AccountId::new([0xFF; 32]),
            initial_stability_fee_per_millisecond: FIXED_POINT_ONE + 1_500_000_000_000_000,
            initial_controller_proportional_gain: -42,
            initial_controller_integral_gain: 123_456,
            initial_minimum_collateralization_ratio: FIXED_POINT_ONE * 3 / 2,
            minimum_milliseconds_between_rate_updates: 300_000,
            maximum_oracle_price_age_milliseconds: 900_000,
            initial_redemption_price: FIXED_POINT_ONE / 2,
            stablecoin_name: "test-stable".to_owned(),
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let decoded: Instruction = serde_json::from_str(&json).expect("deserialize");
        match decoded {
            Instruction::InitializeProgram {
                freeze_authority_account_id,
                initial_stability_fee_per_millisecond,
                initial_controller_proportional_gain,
                initial_controller_integral_gain,
                initial_minimum_collateralization_ratio,
                minimum_milliseconds_between_rate_updates,
                maximum_oracle_price_age_milliseconds,
                initial_redemption_price,
                stablecoin_name,
            } => {
                assert_eq!(freeze_authority_account_id, AccountId::new([0xFF; 32]));
                assert_eq!(
                    initial_stability_fee_per_millisecond,
                    FIXED_POINT_ONE + 1_500_000_000_000_000
                );
                assert_eq!(initial_controller_proportional_gain, -42);
                assert_eq!(initial_controller_integral_gain, 123_456);
                assert_eq!(
                    initial_minimum_collateralization_ratio,
                    FIXED_POINT_ONE * 3 / 2
                );
                assert_eq!(minimum_milliseconds_between_rate_updates, 300_000);
                assert_eq!(maximum_oracle_price_age_milliseconds, 900_000);
                assert_eq!(initial_redemption_price, FIXED_POINT_ONE / 2);
                assert_eq!(stablecoin_name, "test-stable");
            }
            _ => panic!("decoded into wrong variant"),
        }
    }
}
