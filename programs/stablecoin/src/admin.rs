//! Admin parameter updates (spec §10.10–10.16).
//!
//! Every setter shares one shape now, and it is tighter than v0.2.5's. The plan
//! half checks the caller signed, checks the `ProtocolParameters` address, and
//! bound-checks the new value against §8 — all of which it can do, because a
//! §8 band is a pure function of an instruction field. The parameters shard's own
//! apply then checks the caller *is* the stored admin and overwrites exactly the
//! named fields.
//!
//! The §8 bands are therefore asserted once, in the plan, rather than in both
//! halves. The plan always runs before the apply in the same atomic call, so a
//! value the plan rejected never reaches a write.

use lee_core::{
    account::{AccountId, ShardData},
    program::AccountMeta,
};
use stablecoin_core::{ParameterChange, ProtocolParameters, UpdateParameters};

/// Plan half shared by every admin setter.
///
/// # Panics
/// - `admin` did not sign.
/// - `protocol_parameters` is not at its canonical PDA.
pub fn plan_update_parameters(
    admin: &AccountMeta,
    protocol_parameters: &AccountMeta,
    stablecoin_program_id: AccountId,
) {
    crate::checks::assert_admin_signed(admin);
    crate::shared::assert_parameters_slot(protocol_parameters, stablecoin_program_id);
}

/// Plan half of `SetMinimumCollateralizationRatio` (§10.11).
///
/// Tightening leaves existing positions retroactively under-collateralized: they
/// can still deposit or repay to recover, but cannot withdraw or borrow until
/// back above the ratio.
///
/// # Panics
/// As [`plan_update_parameters`], plus a `new_ratio` outside the §8 band.
#[must_use]
pub fn plan_set_minimum_collateralization_ratio(
    admin: &AccountMeta,
    protocol_parameters: &AccountMeta,
    new_ratio: u128,
    stablecoin_program_id: AccountId,
) -> UpdateParameters {
    plan_update_parameters(admin, protocol_parameters, stablecoin_program_id);
    crate::checks::assert_collateralization_ratio_in_band(
        new_ratio,
        "new_minimum_collateralization_ratio",
    );
    UpdateParameters {
        expected_admin: admin.account_id,
        change: ParameterChange::MinimumCollateralizationRatio(new_ratio),
    }
}

/// Plan half of `SetControllerGains` (§10.12). Bundled, and deliberately does not
/// reset the controller's integral term.
///
/// # Panics
/// As [`plan_update_parameters`], plus either gain outside its §8 magnitude cap.
#[must_use]
pub fn plan_set_controller_gains(
    admin: &AccountMeta,
    protocol_parameters: &AccountMeta,
    new_proportional_gain: i128,
    new_integral_gain: i128,
    stablecoin_program_id: AccountId,
) -> UpdateParameters {
    plan_update_parameters(admin, protocol_parameters, stablecoin_program_id);
    crate::checks::assert_controller_gains_in_band(new_proportional_gain, new_integral_gain);
    UpdateParameters {
        expected_admin: admin.account_id,
        change: ParameterChange::ControllerGains {
            proportional: new_proportional_gain,
            integral: new_integral_gain,
        },
    }
}

/// Plan half of `SetTimingParameters` (§10.14). Bundled.
///
/// # Panics
/// As [`plan_update_parameters`], plus either value outside the §8 band.
#[must_use]
pub fn plan_set_timing_parameters(
    admin: &AccountMeta,
    protocol_parameters: &AccountMeta,
    new_minimum_milliseconds_between_rate_updates: u64,
    new_maximum_oracle_price_age_milliseconds: u64,
    stablecoin_program_id: AccountId,
) -> UpdateParameters {
    plan_update_parameters(admin, protocol_parameters, stablecoin_program_id);
    crate::checks::assert_timing_milliseconds_in_band(
        new_minimum_milliseconds_between_rate_updates,
        "new_minimum_milliseconds_between_rate_updates",
    );
    crate::checks::assert_timing_milliseconds_in_band(
        new_maximum_oracle_price_age_milliseconds,
        "new_maximum_oracle_price_age_milliseconds",
    );
    UpdateParameters {
        expected_admin: admin.account_id,
        change: ParameterChange::TimingParameters {
            minimum_milliseconds_between_rate_updates:
                new_minimum_milliseconds_between_rate_updates,
            maximum_oracle_price_age_milliseconds: new_maximum_oracle_price_age_milliseconds,
        },
    }
}

/// Plan half of `SetAdmin` (§10.15).
///
/// One-step and effective immediately, since every admin check reads the stored
/// handle. A wrong id locks the admin out permanently.
///
/// # Panics
/// As [`plan_update_parameters`].
#[must_use]
pub fn plan_set_admin(
    admin: &AccountMeta,
    protocol_parameters: &AccountMeta,
    new_admin_account_id: AccountId,
    stablecoin_program_id: AccountId,
) -> UpdateParameters {
    plan_update_parameters(admin, protocol_parameters, stablecoin_program_id);
    UpdateParameters {
        expected_admin: admin.account_id,
        change: ParameterChange::Admin(new_admin_account_id),
    }
}

/// Plan half of `SetFreezeAuthority` (§10.16).
///
/// Set by the **admin**, not by the freeze authority, so a compromised freeze
/// authority cannot entrench itself.
///
/// # Panics
/// As [`plan_update_parameters`].
#[must_use]
pub fn plan_set_freeze_authority(
    admin: &AccountMeta,
    protocol_parameters: &AccountMeta,
    new_freeze_authority_account_id: AccountId,
    stablecoin_program_id: AccountId,
) -> UpdateParameters {
    plan_update_parameters(admin, protocol_parameters, stablecoin_program_id);
    UpdateParameters {
        expected_admin: admin.account_id,
        change: ParameterChange::FreezeAuthority(new_freeze_authority_account_id),
    }
}

/// Plan half of `SetMarketPriceOracle` (§10.13).
///
/// The replacement must quote the same pair as the one bound at bootstrap, and
/// the pin chain for that runs through two shards: the pair this declares is
/// checked against the stored definitions by the parameters apply, and against
/// the replacement's own fields by a guard on the oracle shard. Neither apply
/// sees both accounts, and between them they establish what the single
/// v0.2.5 function established directly.
///
/// The replacement's shard key is recorded too — see
/// [`ProtocolParameters::market_price_oracle_program_id`]. The producing program
/// is still unconstrained; it is simply bound by the admin here instead of being
/// chosen per call later.
///
/// # Panics
/// As [`plan_update_parameters`].
#[must_use]
pub fn plan_set_market_price_oracle(
    admin: &AccountMeta,
    protocol_parameters: &AccountMeta,
    new_oracle: &AccountMeta,
    expected_base_asset: AccountId,
    expected_quote_asset: AccountId,
    stablecoin_program_id: AccountId,
) -> UpdateParameters {
    plan_update_parameters(admin, protocol_parameters, stablecoin_program_id);
    UpdateParameters {
        expected_admin: admin.account_id,
        change: ParameterChange::MarketPriceOracle {
            new_oracle_id: new_oracle.account_id,
            new_oracle_program_id: new_oracle.program_account_id,
            expected_base_asset,
            expected_quote_asset,
        },
    }
}

/// Plan half of `SetStabilityFeePerMillisecond` (§10.10).
///
/// The only setter that takes the accumulator and the clock, because the fee
/// accrues forward at the **old** rate first so the new one is never
/// retroactive. That accrual happens in the accumulator's own shard, which cannot
/// see the parameters — so the old rate travels there as a declared value, and
/// `ParameterChange::StabilityFee::old_rate` is what pins it against the record
/// being overwritten.
///
/// # Panics
/// As [`plan_update_parameters`], plus a `new_rate` outside the §8 band, either
/// global not at its canonical PDA, or a non-canonical clock.
#[must_use]
pub fn plan_set_stability_fee_per_millisecond(
    admin: &AccountMeta,
    protocol_parameters: &AccountMeta,
    stability_fee_accumulator: &AccountMeta,
    clock: &AccountMeta,
    old_rate: u128,
    new_rate: u128,
    stablecoin_program_id: AccountId,
) -> UpdateParameters {
    plan_update_parameters(admin, protocol_parameters, stablecoin_program_id);
    crate::shared::assert_accumulator_slot(stability_fee_accumulator, stablecoin_program_id);
    crate::shared::assert_canonical_clock(clock);
    crate::checks::assert_stability_fee_in_band(new_rate, "new_stability_fee_per_millisecond");
    UpdateParameters {
        expected_admin: admin.account_id,
        change: ParameterChange::StabilityFee { old_rate, new_rate },
    }
}

/// Apply half shared by every admin setter: gate on the stored admin, then
/// overwrite exactly the named fields.
///
/// §8 bands are not re-checked here; they are pure functions of instruction
/// fields and are asserted in the plan, which always runs first.
///
/// # Panics
/// - the shard is not a valid [`ProtocolParameters`] (the protocol is not bootstrapped).
/// - the stored admin is not the account the plan saw sign.
/// - for `MarketPriceOracle`, the declared pair is not the bound definitions.
/// - for `StabilityFee`, `old_rate` is not the stored rate.
#[must_use]
pub fn update_parameters(pre_data: &ShardData, effect: &UpdateParameters) -> ShardData {
    let mut parameters = ProtocolParameters::try_from(pre_data)
        .expect("ProtocolParameters account must be initialized and decode");
    crate::checks::assert_is_stored_admin(parameters.admin_account_id, effect.expected_admin);

    match effect.change {
        ParameterChange::MinimumCollateralizationRatio(new_ratio) => {
            parameters.minimum_collateralization_ratio = new_ratio;
        }
        ParameterChange::ControllerGains {
            proportional,
            integral,
        } => {
            parameters.controller_proportional_gain = proportional;
            parameters.controller_integral_gain = integral;
        }
        ParameterChange::TimingParameters {
            minimum_milliseconds_between_rate_updates,
            maximum_oracle_price_age_milliseconds,
        } => {
            parameters.minimum_milliseconds_between_rate_updates =
                minimum_milliseconds_between_rate_updates;
            parameters.maximum_oracle_price_age_milliseconds =
                maximum_oracle_price_age_milliseconds;
        }
        ParameterChange::Admin(new_admin_account_id) => {
            parameters.admin_account_id = new_admin_account_id;
        }
        ParameterChange::FreezeAuthority(new_freeze_authority_account_id) => {
            parameters.freeze_authority_account_id = new_freeze_authority_account_id;
        }
        ParameterChange::MarketPriceOracle {
            new_oracle_id,
            new_oracle_program_id,
            expected_base_asset,
            expected_quote_asset,
        } => {
            // Half of the pair check. The other half — that the replacement
            // really quotes this pair — is a guard on the oracle's own shard.
            assert_eq!(
                parameters.stablecoin_definition_id, expected_base_asset,
                "New oracle base_asset must equal the stablecoin definition's account_id"
            );
            assert_eq!(
                parameters.collateral_definition_id, expected_quote_asset,
                "New oracle quote_asset must equal the collateral definition's account_id"
            );
            parameters.market_price_oracle_id = new_oracle_id;
            parameters.market_price_oracle_program_id = new_oracle_program_id;
        }
        ParameterChange::StabilityFee { old_rate, new_rate } => {
            // Pins the rate the accumulator's own effect is accruing at, which
            // is the whole of what makes the change non-retroactive.
            assert_eq!(
                parameters.stability_fee_per_millisecond, old_rate,
                "Declared current stability fee does not match the ProtocolParameters account"
            );
            parameters.stability_fee_per_millisecond = new_rate;
        }
    }

    ShardData::from(&parameters)
}
