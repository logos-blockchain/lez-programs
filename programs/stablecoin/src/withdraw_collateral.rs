//! `Instruction::WithdrawCollateral` (spec §10.6).

use lee_core::{
    account::{AccountId, ShardData},
    program::{AccountMeta, ChainedCall},
};
use stablecoin_core::{Position, ProtocolParameters, WithdrawCollateral};

/// Plan half: check the addresses and chain the vault→user transfer.
///
/// Blocked while frozen (§7): a withdrawal is the one collateral move that can
/// only worsen the protocol's position.
///
/// # Panics
/// - `owner` did not sign.
/// - any global is not at its canonical PDA, or the clock is not `CLOCK_01`.
/// - the protocol is frozen.
/// - any token slot does not select the bound Token Program's shard.
/// - `position` or `vault` is not at its PDA derivation.
#[expect(
    clippy::too_many_arguments,
    reason = "the account inputs mirror the spec §10.6 ABI, plus the declared globals and clock"
)]
#[must_use]
pub fn plan_withdraw_collateral(
    owner: &AccountMeta,
    position: &AccountMeta,
    vault: &AccountMeta,
    user_collateral_holding: &AccountMeta,
    stability_fee_accumulator: &AccountMeta,
    redemption_price_state: &AccountMeta,
    protocol_parameters: &AccountMeta,
    clock: &AccountMeta,
    parameters: &ProtocolParameters,
    position_nonce: u64,
    amount: u128,
    stablecoin_program_id: AccountId,
) -> ChainedCall {
    assert!(owner.is_authorized, "Owner authorization is missing");
    crate::shared::assert_parameters_slot(protocol_parameters, stablecoin_program_id);
    crate::shared::assert_accumulator_slot(stability_fee_accumulator, stablecoin_program_id);
    crate::shared::assert_redemption_slot(redemption_price_state, stablecoin_program_id);
    crate::shared::assert_canonical_clock(clock);
    assert!(!parameters.is_frozen, "Protocol is frozen");
    crate::shared::assert_token_program(vault, parameters, "Position vault");
    crate::shared::assert_token_program(
        user_collateral_holding,
        parameters,
        "User collateral holding",
    );

    let vault_seed = crate::shared::assert_position_and_vault_slots(
        position,
        vault,
        owner,
        position_nonce,
        stablecoin_program_id,
    );

    // The vault is the sender, and a chained call ships shard selectors rather
    // than authorized accounts — so the vault's PDA seed is the whole of the
    // authority for debiting it.
    crate::shared::transfer_call(
        parameters.token_program_id,
        vault.account_id,
        user_collateral_holding.account_id,
        crate::shared::collateral_descriptor(parameters),
        amount,
    )
    .with_pda_seeds(vec![vault_seed])
}

/// Apply half: shrink the position's collateral and re-check §6.2.
///
/// The check is against debt and redemption price both projected forward to the
/// pinned `now` (§5.3) — see
/// [`assert_collateralized`](crate::generate_debt::assert_collateralized), which
/// this shares with `GenerateDebt`.
///
/// It is skipped when either `amount` or the position's normalized debt is zero.
/// A debt-free position has no invariant to satisfy, and a zero-amount
/// withdrawal changes nothing — so neither needs a live redemption price, and
/// requiring one would make both fail on a stale oracle for no benefit. §6.2 is
/// an *inequality on the new state*, so skipping it where the state is trivially
/// safe is sound rather than lenient.
///
/// # Panics
/// - the shard is not a valid [`Position`], or its identity differs from the declared one.
/// - `amount` exceeds the recorded collateral.
/// - §6.2 fails after the withdrawal.
#[must_use]
pub fn position_after_withdraw(pre_data: &ShardData, effect: &WithdrawCollateral) -> ShardData {
    let position = crate::shared::position_of(pre_data, &effect.expected);
    let collateral_amount = position
        .collateral_amount
        .checked_sub(effect.amount)
        .expect("Withdraw amount exceeds position collateral");

    let updated = Position {
        collateral_amount,
        ..position
    };
    if effect.amount != 0 && updated.normalized_debt_amount != 0 {
        crate::generate_debt::assert_collateralized(&updated, &effect.inputs);
    }
    ShardData::from(&updated)
}
