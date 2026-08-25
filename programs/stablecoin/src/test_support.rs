//! Shared account builders for the poke host-function unit tests.
//!
//! The three pokes (`accrue_stability_fee`, `update_redemption_rate`,
//! `refresh_globals`) validate overlapping account sets, so their tests would
//! otherwise repeat the same fixtures three times.

use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, Data, Nonce},
    program::ProgramId,
};
use stablecoin_core::{
    compute_protocol_parameters_pda, compute_redemption_price_state_pda,
    compute_stability_fee_accumulator_pda, math::FIXED_POINT_ONE, ProtocolParameters,
    RedemptionPriceState, StabilityFeeAccumulator,
};
use twap_oracle_core::OraclePriceAccount;

pub(crate) const STABLECOIN_PROGRAM_ID: ProgramId = [3u32; 8];
pub(crate) const ORACLE_PROGRAM_ID: ProgramId = [4u32; 8];
pub(crate) const CLOCK_PROGRAM_ID: ProgramId = [5u32; 8];

/// Timestamps are Unix milliseconds, matching the `CLOCK_01` account.
pub(crate) const T0: u64 = 1_700_000_000_000;
/// Ten minutes after `T0` — past the 300_000 ms rate-update interval below.
pub(crate) const NOW: u64 = T0 + 600_000;

/// ~5% annual in per-millisecond fixed-point, the same realistic rate the
/// `initialize_program` tests use.
pub(crate) const TEST_STABILITY_FEE_PER_MILLISECOND: u128 = FIXED_POINT_ONE + 1_500_000_000_000_000;
pub(crate) const TEST_MINIMUM_MILLISECONDS_BETWEEN_RATE_UPDATES: u64 = 300_000;
pub(crate) const TEST_MAXIMUM_ORACLE_PRICE_AGE_MILLISECONDS: u64 = 900_000;

/// Starting accumulator anchor: `1.0`.
pub(crate) const ACCUMULATOR_ANCHOR: u128 = FIXED_POINT_ONE;
/// Starting redemption price: `0.5` collateral per stablecoin.
pub(crate) const REDEMPTION_PRICE_ANCHOR: u128 = FIXED_POINT_ONE / 2;
/// A market price below the redemption anchor, so `error > 0` and a positive
/// `Kp` drives the rate above `FIXED_POINT_ONE`.
pub(crate) const MARKET_PRICE_BELOW_ANCHOR: u128 = FIXED_POINT_ONE / 4;

/// Knobs the poke tests actually vary. Everything else is fixed.
pub(crate) struct ParameterOverrides {
    pub(crate) stability_fee_per_millisecond: u128,
    pub(crate) controller_proportional_gain: i128,
    pub(crate) controller_integral_gain: i128,
    pub(crate) is_frozen: bool,
}

impl Default for ParameterOverrides {
    fn default() -> Self {
        Self {
            stability_fee_per_millisecond: TEST_STABILITY_FEE_PER_MILLISECOND,
            controller_proportional_gain: FIXED_POINT_ONE as i128,
            controller_integral_gain: 0,
            is_frozen: false,
        }
    }
}

pub(crate) fn caller_id() -> AccountId {
    AccountId::new([0xCA; 32])
}
pub(crate) fn admin_id() -> AccountId {
    AccountId::new([0xA0; 32])
}
pub(crate) fn freeze_authority_id() -> AccountId {
    AccountId::new([0xFE; 32])
}
pub(crate) fn stablecoin_definition_id() -> AccountId {
    AccountId::new([0x10; 32])
}
pub(crate) fn collateral_definition_id() -> AccountId {
    AccountId::new([0x20; 32])
}
pub(crate) fn oracle_id() -> AccountId {
    AccountId::new([0x30; 32])
}
pub(crate) fn oracle_source_id() -> AccountId {
    AccountId::new([0x31; 32])
}

pub(crate) fn protocol_parameters_id() -> AccountId {
    compute_protocol_parameters_pda(STABLECOIN_PROGRAM_ID)
}
pub(crate) fn accumulator_id() -> AccountId {
    compute_stability_fee_accumulator_pda(STABLECOIN_PROGRAM_ID)
}
pub(crate) fn redemption_price_state_id() -> AccountId {
    compute_redemption_price_state_pda(STABLECOIN_PROGRAM_ID)
}

/// An authorized caller. Pokes are permissionless, so any signer works.
pub(crate) fn caller_account() -> AccountWithMetadata {
    AccountWithMetadata {
        account: Account::default(),
        is_authorized: true,
        account_id: caller_id(),
    }
}

/// An uninitialized account at `account_id` — the shape every "must be
/// initialized" assertion rejects.
pub(crate) fn uninitialized(account_id: AccountId) -> AccountWithMetadata {
    AccountWithMetadata {
        account: Account::default(),
        is_authorized: false,
        account_id,
    }
}

pub(crate) fn protocol_parameters_account(overrides: ParameterOverrides) -> AccountWithMetadata {
    AccountWithMetadata {
        account: Account {
            program_owner: STABLECOIN_PROGRAM_ID,
            balance: 0,
            data: Data::from(&ProtocolParameters {
                admin_account_id: admin_id(),
                freeze_authority_account_id: freeze_authority_id(),
                stablecoin_definition_id: stablecoin_definition_id(),
                collateral_definition_id: collateral_definition_id(),
                market_price_oracle_id: oracle_id(),
                stability_fee_per_millisecond: overrides.stability_fee_per_millisecond,
                controller_proportional_gain: overrides.controller_proportional_gain,
                controller_integral_gain: overrides.controller_integral_gain,
                minimum_collateralization_ratio: FIXED_POINT_ONE * 3 / 2,
                minimum_milliseconds_between_rate_updates:
                    TEST_MINIMUM_MILLISECONDS_BETWEEN_RATE_UPDATES,
                maximum_oracle_price_age_milliseconds: TEST_MAXIMUM_ORACLE_PRICE_AGE_MILLISECONDS,
                is_frozen: overrides.is_frozen,
            }),
            nonce: Nonce(0),
        },
        is_authorized: false,
        account_id: protocol_parameters_id(),
    }
}

pub(crate) fn accumulator_account(anchor: u128, last_accrued_at: u64) -> AccountWithMetadata {
    AccountWithMetadata {
        account: Account {
            program_owner: STABLECOIN_PROGRAM_ID,
            balance: 0,
            data: Data::from(&StabilityFeeAccumulator {
                accumulated_rate_at_last_accrual: anchor,
                last_accrued_at,
            }),
            nonce: Nonce(0),
        },
        is_authorized: false,
        account_id: accumulator_id(),
    }
}

pub(crate) fn redemption_price_state_account(last_updated_at: u64) -> AccountWithMetadata {
    redemption_price_state_account_with_integral_term(last_updated_at, 0)
}

pub(crate) fn redemption_price_state_account_with_integral_term(
    last_updated_at: u64,
    controller_integral_term: i128,
) -> AccountWithMetadata {
    AccountWithMetadata {
        account: Account {
            program_owner: STABLECOIN_PROGRAM_ID,
            balance: 0,
            data: Data::from(&RedemptionPriceState {
                redemption_price_at_last_update: REDEMPTION_PRICE_ANCHOR,
                redemption_rate_per_millisecond: FIXED_POINT_ONE,
                controller_integral_term,
                last_updated_at,
            }),
            nonce: Nonce(0),
        },
        is_authorized: false,
        account_id: redemption_price_state_id(),
    }
}

pub(crate) fn oracle_account(timestamp: u64, price: u128) -> AccountWithMetadata {
    AccountWithMetadata {
        account: Account {
            program_owner: ORACLE_PROGRAM_ID,
            balance: 0,
            data: Data::from(&OraclePriceAccount {
                base_asset: stablecoin_definition_id(),
                quote_asset: collateral_definition_id(),
                price,
                timestamp,
                source_id: oracle_source_id(),
                confidence_interval: 0,
            }),
            nonce: Nonce(0),
        },
        is_authorized: false,
        account_id: oracle_id(),
    }
}

pub(crate) fn clock_account(timestamp: u64) -> AccountWithMetadata {
    AccountWithMetadata {
        account: Account {
            program_owner: CLOCK_PROGRAM_ID,
            balance: 0,
            data: Data::try_from(
                ClockAccountData {
                    block_id: 0,
                    timestamp,
                }
                .to_bytes(),
            )
            .expect("clock data fits"),
            nonce: Nonce(0),
        },
        is_authorized: false,
        account_id: CLOCK_01_PROGRAM_ACCOUNT_ID,
    }
}
