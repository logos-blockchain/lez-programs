//! Single-instance configuration account for the Stablecoin program.
//!
//! Created at [`initialize_program`] time, written by admin `set_*`
//! instructions and by `freeze` / `unfreeze`. Read by nearly every other
//! instruction.

use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::{
    account::{AccountId, Data},
    program::PdaSeed,
};
use serde::{Deserialize, Serialize};
use spel_framework_macros::account_type;

/// PDA seed for the single-instance [`ProtocolParameters`] account.
const PROTOCOL_PARAMETERS_PDA_DOMAIN: &[u8; 32] = b"STABLECOIN__PROTOCOL_PARAMS_____";

/// Single source of truth for the protocol's configurable knobs.
///
/// See the spec, §4.1 for field semantics. Immutability comments mirror the
/// rationale documented there.
#[account_type]
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct ProtocolParameters {
    /// Account layout version, must equal `GLOBAL_SCHEMA_VERSION`.
    pub schema_version: u8,
    /// Authority required for every parameter-update and admin-rotation
    /// instruction.
    pub admin_account_id: AccountId,
    /// Authority required for `freeze` / `unfreeze`.
    pub freeze_authority_account_id: AccountId,
    /// The stablecoin's `TokenDefinition` PDA. IMMUTABLE — changing it would
    /// break supply accounting against the existing on-chain stablecoin
    /// float.
    pub stablecoin_definition_id: AccountId,
    /// The single accepted collateral's `TokenDefinition`. IMMUTABLE —
    /// changing it would orphan every position vault.
    pub collateral_definition_id: AccountId,
    /// Producer of the market price observation used by
    /// `update_redemption_rate` and the oracle staleness gate in
    /// `generate_debt`.
    pub market_price_oracle_id: AccountId,
    /// Per-millisecond stability fee multiplier in fixed-point, stored as
    /// `(1 + r_per_millisecond) * FIXED_POINT_ONE`.
    pub stability_fee_per_millisecond: u128,
    /// PI controller `Kp`. Signed.
    pub controller_proportional_gain: i128,
    /// PI controller `Ki`. Signed.
    pub controller_integral_gain: i128,
    /// Minimum collateralization ratio in fixed-point (e.g.
    /// `1.5 * FIXED_POINT_ONE` = 150%).
    pub minimum_collateralization_ratio: u128,
    /// Minimum milliseconds between successful `update_redemption_rate` calls.
    pub minimum_milliseconds_between_rate_updates: u64,
    /// Reject oracle observations older than this (RFP R3 staleness gate).
    pub maximum_oracle_price_age_milliseconds: u64,
    /// `true` blocks `open_position`, `generate_debt`, `withdraw_collateral`.
    pub is_frozen: bool,
    /// Minimum positive accrued position debt, in raw stablecoin atomic units.
    pub minimum_debt_amount: u128,
    /// Aggregate accrued debt ceiling for new borrowing, in raw atomic units.
    pub debt_ceiling: u128,
}

impl TryFrom<&Data> for ProtocolParameters {
    type Error = std::io::Error;

    fn try_from(data: &Data) -> Result<Self, Self::Error> {
        if data.as_ref().len() == 241 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "Legacy stablecoin global account schema is unsupported",
            ));
        }
        let decoded = Self::try_from_slice(data.as_ref())?;
        if decoded.schema_version != crate::GLOBAL_SCHEMA_VERSION {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "Unsupported stablecoin global account schema",
            ));
        }
        Ok(decoded)
    }
}

impl From<&ProtocolParameters> for Data {
    fn from(params: &ProtocolParameters) -> Self {
        let len = borsh::object_length(params).expect("ProtocolParameters length must be known");
        let mut buf = Vec::with_capacity(len);
        BorshSerialize::serialize(params, &mut buf)
            .expect("ProtocolParameters serialization should not fail");
        Self::try_from(buf).expect("ProtocolParameters encoded data should fit into Data")
    }
}

/// PDA seed for [`ProtocolParameters`] (single-instance — no per-deployment
/// uniqueness needed; the program id already discriminates).
#[must_use]
pub fn compute_protocol_parameters_pda_seed() -> PdaSeed {
    use risc0_zkvm::sha::{Impl, Sha256 as _};

    let mut out = [0u8; 32];
    out.copy_from_slice(Impl::hash_bytes(PROTOCOL_PARAMETERS_PDA_DOMAIN).as_bytes());
    PdaSeed::new(out)
}

/// Account id of the [`ProtocolParameters`] PDA under the given stablecoin
/// program.
#[must_use]
pub fn compute_protocol_parameters_pda(stablecoin_program_id: AccountId) -> AccountId {
    AccountId::for_public_pda(
        &stablecoin_program_id,
        &compute_protocol_parameters_pda_seed(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::FIXED_POINT_ONE;

    fn sample_params() -> ProtocolParameters {
        ProtocolParameters {
            schema_version: crate::GLOBAL_SCHEMA_VERSION,
            minimum_debt_amount: 10,
            debt_ceiling: 1_000_000,
            admin_account_id: AccountId::new([0xAA; 32]),
            freeze_authority_account_id: AccountId::new([0xFF; 32]),
            stablecoin_definition_id: AccountId::new([0x01; 32]),
            collateral_definition_id: AccountId::new([0x02; 32]),
            market_price_oracle_id: AccountId::new([0x03; 32]),
            stability_fee_per_millisecond: FIXED_POINT_ONE + 1_500_000_000_000_000, /* ε ≈ 1.5×10^15 → ~5% annual */
            controller_proportional_gain: -123_456_789,
            controller_integral_gain: 987_654_321,
            minimum_collateralization_ratio: FIXED_POINT_ONE * 3 / 2,
            minimum_milliseconds_between_rate_updates: 300_000,
            maximum_oracle_price_age_milliseconds: 900_000,
            is_frozen: false,
        }
    }

    #[test]
    fn borsh_roundtrip_preserves_every_field() {
        let params = sample_params();
        let data: Data = (&params).into();
        let decoded = ProtocolParameters::try_from(&data).expect("decode");
        assert_eq!(decoded, params);
    }

    #[test]
    fn borsh_roundtrip_with_is_frozen_true() {
        let mut params = sample_params();
        params.is_frozen = true;
        let data: Data = (&params).into();
        let decoded = ProtocolParameters::try_from(&data).expect("decode");
        assert!(decoded.is_frozen);
        assert_eq!(decoded, params);
    }

    #[test]
    fn pda_is_deterministic_for_fixed_program_id() {
        let program_id = AccountId::new([42u8; 32]);
        let first = compute_protocol_parameters_pda(program_id);
        let second = compute_protocol_parameters_pda(program_id);
        assert_eq!(first, second);
    }

    #[test]
    fn pda_differs_for_different_program_ids() {
        let id_a = AccountId::new([1u8; 32]);
        let id_b = AccountId::new([2u8; 32]);
        assert_ne!(
            compute_protocol_parameters_pda(id_a),
            compute_protocol_parameters_pda(id_b),
        );
    }

    #[test]
    fn seed_is_deterministic() {
        let seed_a = compute_protocol_parameters_pda_seed();
        let seed_b = compute_protocol_parameters_pda_seed();
        assert_eq!(seed_a, seed_b);
    }
    #[test]
    fn canonical_v2_layout_and_strict_decoding() {
        let params = sample_params();
        let bytes = borsh::to_vec(&params).expect("encode");
        let mut expected = vec![2];
        for byte in [0xAA, 0xFF, 0x01, 0x02, 0x03] {
            expected.extend_from_slice(&[byte; 32]);
        }
        expected.extend_from_slice(&(FIXED_POINT_ONE + 1_500_000_000_000_000).to_le_bytes());
        expected.extend_from_slice(&(-123_456_789_i128).to_le_bytes());
        expected.extend_from_slice(&987_654_321_i128.to_le_bytes());
        expected.extend_from_slice(&(FIXED_POINT_ONE * 3 / 2).to_le_bytes());
        expected.extend_from_slice(&300_000_u64.to_le_bytes());
        expected.extend_from_slice(&900_000_u64.to_le_bytes());
        expected.push(0);
        expected.extend_from_slice(&10_u128.to_le_bytes());
        expected.extend_from_slice(&1_000_000_u128.to_le_bytes());
        assert_eq!(bytes.len(), 274);
        assert_eq!(bytes, expected);

        let legacy = Data::try_from(bytes[1..242].to_vec()).expect("data");
        assert_eq!(
            ProtocolParameters::try_from(&legacy)
                .expect_err("legacy")
                .kind(),
            std::io::ErrorKind::Unsupported
        );
        let mut unsupported = bytes.clone();
        unsupported[0] = 3;
        assert_eq!(
            ProtocolParameters::try_from(&Data::try_from(unsupported).expect("data"))
                .expect_err("version")
                .kind(),
            std::io::ErrorKind::Unsupported
        );
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(ProtocolParameters::try_from(&Data::try_from(trailing).expect("data")).is_err());
        assert!(ProtocolParameters::try_from(
            &Data::try_from(bytes[..273].to_vec()).expect("data")
        )
        .is_err());
    }
}
