//! `Instruction::DepositCollateral` (spec §10.5).

use lee_core::{
    account::{AccountId, ShardData},
    program::{AccountMeta, ChainedCall},
};
use stablecoin_core::{DepositCollateral, Position, ProtocolParameters};

/// Plan half: check the addresses and chain the transfer into the vault.
///
/// Allowed while frozen, and deliberately without a §6.2 check — a deposit can
/// only improve the position's collateralization, so spec §7 keeps this path
/// open in emergencies. `is_frozen` is therefore read from the declared
/// parameters by nobody; the parameters guard still pins the record, which is
/// what binds `collateral_definition_id` and `token_program_id`.
///
/// # Panics
/// - `owner` or `user_collateral_holding` did not sign.
/// - `protocol_parameters` is not at its canonical PDA.
/// - either token slot does not select the bound Token Program's shard.
/// - `position` or `vault` is not at its PDA derivation.
#[expect(
    clippy::too_many_arguments,
    reason = "the account inputs mirror the spec §10.5 ABI, plus the declared parameters"
)]
#[must_use]
pub fn plan_deposit_collateral(
    owner: &AccountMeta,
    position: &AccountMeta,
    vault: &AccountMeta,
    user_collateral_holding: &AccountMeta,
    protocol_parameters: &AccountMeta,
    parameters: &ProtocolParameters,
    position_nonce: u64,
    amount: u128,
    stablecoin_program_id: AccountId,
) -> ChainedCall {
    assert!(owner.is_authorized, "Owner authorization is missing");
    assert!(
        user_collateral_holding.is_authorized,
        "User collateral holding authorization is missing"
    );
    crate::shared::assert_parameters_slot(protocol_parameters, stablecoin_program_id);
    crate::shared::assert_token_program(vault, parameters, "Position vault");
    crate::shared::assert_token_program(
        user_collateral_holding,
        parameters,
        "User collateral holding",
    );
    crate::shared::assert_position_and_vault_slots(
        position,
        vault,
        owner,
        position_nonce,
        stablecoin_program_id,
    );

    // No PDA seed: the sender is the user's own holding, authorized by the
    // transaction's witness set, and the receiving vault needs no authorization.
    crate::shared::transfer_call(
        parameters.token_program_id,
        user_collateral_holding.account_id,
        vault.account_id,
        crate::shared::collateral_descriptor(parameters),
        amount,
    )
}

/// Apply half: reconcile the position's collateral from the vault.
///
/// Set to the vault's balance plus `amount`, not incremented. `Token::Transfer`
/// only needs the sender's authorization, so anyone can donate straight into a
/// vault; recording `position + amount` would leave the position permanently
/// below the vault and strand the difference, since withdrawals are bounded by
/// the recorded figure. A zero-amount deposit is the recovery path for exactly
/// that case.
///
/// `vault_balance` is the pre-transfer figure, which is the one the
/// reconciliation wants: every effect of a call runs before any of its chained
/// calls, so the guard that pinned it saw the vault before the deposit landed.
///
/// # Panics
/// - the shard is not a valid [`Position`], or its identity differs from the declared one.
/// - the vault balance plus `amount` overflows.
#[must_use]
pub fn position_after_deposit(pre_data: &ShardData, effect: &DepositCollateral) -> ShardData {
    let position = crate::shared::position_of(pre_data, &effect.expected);
    let collateral_amount = effect
        .vault_balance
        .checked_add(effect.amount)
        .expect("Position collateral_amount overflow");
    ShardData::from(&Position {
        collateral_amount,
        ..position
    })
}
