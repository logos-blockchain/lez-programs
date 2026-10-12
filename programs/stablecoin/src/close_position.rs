//! `Instruction::ClosePosition` (spec §10.9).

use lee_core::{
    account::{AccountId, ShardData},
    program::AccountMeta,
};
use stablecoin_core::{ClosePosition, ProtocolParameters};

/// Plan half: check the addresses. Allowed while frozen — closing a settled
/// position removes an obligation and cannot worsen protocol health.
///
/// # Panics
/// - `owner` did not sign.
/// - `protocol_parameters` is not at its canonical PDA.
/// - the vault slot does not select the bound Token Program's shard.
/// - `position` or `vault` is not at its PDA derivation.
pub fn plan_close_position(
    owner: &AccountMeta,
    position: &AccountMeta,
    vault: &AccountMeta,
    protocol_parameters: &AccountMeta,
    parameters: &ProtocolParameters,
    position_nonce: u64,
    stablecoin_program_id: AccountId,
) {
    assert!(owner.is_authorized, "Owner authorization is missing");
    crate::shared::assert_parameters_slot(protocol_parameters, stablecoin_program_id);
    crate::shared::assert_token_program(vault, parameters, "Position vault");
    crate::shared::assert_position_and_vault_slots(
        position,
        vault,
        owner,
        position_nonce,
        stablecoin_program_id,
    );
}

/// Apply half: clear a fully-settled position's shard.
///
/// v0.2.5 could only zero the *data*: a diff carried no `program_owner`, the
/// runtime derived it, and the account lingered stablecoin-owned — which is why
/// §10.9's `Account::default()` was unexpressible and the `(owner,
/// position_nonce)` pair could never be reused. v0.3.0 removes ownership from an
/// address entirely, so clearing the shard *is* the close, and the pair becomes
/// reusable: `OpenPosition`'s `create` asks only that the shard be empty.
///
/// The vault still lingers at `balance = 0` — the Token Program has no
/// `CloseHolding` (§12) — and its emptiness is asserted by a guard on its own
/// shard rather than read here. That catches the case where the two disagree,
/// which would strand tokens behind a cleared position.
///
/// # Panics
/// - the shard is not a valid `Position`, or its identity differs from the declared one.
/// - the position still has debt or collateral recorded.
#[must_use]
pub fn position_after_close(pre_data: &ShardData, effect: &ClosePosition) -> ShardData {
    let position = crate::shared::position_of(pre_data, &effect.expected);
    assert_eq!(
        position.normalized_debt_amount, 0,
        "Position still has outstanding debt"
    );
    assert_eq!(
        position.collateral_amount, 0,
        "Position still holds collateral"
    );
    ShardData::default()
}
