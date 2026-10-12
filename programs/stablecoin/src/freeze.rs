//! Emergency kill switch: `Freeze` / `Unfreeze` (spec §10.17, §10.18).
//!
//! While frozen, `OpenPosition`, `WithdrawCollateral` and `GenerateDebt` are
//! refused; `DepositCollateral`, `RepayDebt`, `ClosePosition` and the
//! permissionless pokes keep working (§7). Idempotent in both directions.

use lee_core::{
    account::{AccountId, ShardData},
    program::AccountMeta,
};
use stablecoin_core::{ProtocolParameters, SetFrozen};

/// Plan half of `Freeze` / `Unfreeze`.
///
/// The gate is the freeze-authority handle, not the admin — so this does not go
/// through [`plan_update_parameters`](crate::admin::plan_update_parameters). Which
/// account currently holds that handle is stored in the parameters shard, so the
/// plan reports who claims it and [`set_frozen`] does the comparison.
///
/// # Panics
/// - `freeze_authority` did not sign.
/// - `protocol_parameters` is not at its canonical PDA.
#[must_use]
pub fn plan_set_frozen(
    freeze_authority: &AccountMeta,
    protocol_parameters: &AccountMeta,
    is_frozen: bool,
    stablecoin_program_id: AccountId,
) -> SetFrozen {
    assert!(
        freeze_authority.is_authorized,
        "Freeze authority authorization is missing"
    );
    crate::shared::assert_parameters_slot(protocol_parameters, stablecoin_program_id);
    SetFrozen {
        expected_freeze_authority: freeze_authority.account_id,
        is_frozen,
    }
}

/// Apply half: gate on the stored freeze authority, then set the flag.
///
/// # Panics
/// - the shard is not a valid [`ProtocolParameters`].
/// - the stored freeze authority is not the account the plan saw sign.
#[must_use]
pub fn set_frozen(pre_data: &ShardData, effect: &SetFrozen) -> ShardData {
    let mut parameters = ProtocolParameters::try_from(pre_data)
        .expect("ProtocolParameters account must be initialized and decode");
    assert_eq!(
        parameters.freeze_authority_account_id, effect.expected_freeze_authority,
        "Signer is not the protocol's freeze authority"
    );
    parameters.is_frozen = effect.is_frozen;
    ShardData::from(&parameters)
}
