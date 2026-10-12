//! `Instruction::RepayDebt` (spec §10.8).

use lee_core::{
    account::{AccountId, ProgramShardSelector, ShardData},
    program::{AccountMeta, ChainedCall},
};
use stablecoin_core::{
    math::{mul_div, FIXED_POINT_ONE},
    Position, ProtocolParameters, RepayDebt,
};

/// Plan half: check the addresses and chain the `Token::Burn`.
///
/// Allowed while frozen — repaying only improves the protocol's position (§7) —
/// and with no collateralization check, for the same reason.
///
/// # Panics
/// - `owner` or `user_stablecoin_holding` did not sign.
/// - any global is not at its canonical PDA, or the clock is not `CLOCK_01`.
/// - `stablecoin_definition` is not the bound definition, or any token slot does not select the
///   bound Token Program's shard.
/// - `position` is not at its `(owner, position_nonce)` PDA.
#[expect(
    clippy::too_many_arguments,
    reason = "the account inputs mirror the spec §10.8 ABI, plus the declared globals and clock"
)]
#[must_use]
pub fn plan_repay_debt(
    owner: &AccountMeta,
    position: &AccountMeta,
    stablecoin_definition: &AccountMeta,
    user_stablecoin_holding: &AccountMeta,
    stability_fee_accumulator: &AccountMeta,
    protocol_parameters: &AccountMeta,
    clock: &AccountMeta,
    parameters: &ProtocolParameters,
    position_nonce: u64,
    amount: u128,
    stablecoin_program_id: AccountId,
) -> ChainedCall {
    assert!(owner.is_authorized, "Owner authorization is missing");
    assert!(
        user_stablecoin_holding.is_authorized,
        "User stablecoin holding authorization is missing"
    );
    crate::shared::assert_parameters_slot(protocol_parameters, stablecoin_program_id);
    crate::shared::assert_accumulator_slot(stability_fee_accumulator, stablecoin_program_id);
    crate::shared::assert_canonical_clock(clock);
    assert_eq!(
        stablecoin_definition.account_id, parameters.stablecoin_definition_id,
        "Stablecoin definition does not match the one bound at initialize_program"
    );
    crate::shared::assert_token_program(stablecoin_definition, parameters, "Stablecoin definition");
    crate::shared::assert_token_program(
        user_stablecoin_holding,
        parameters,
        "User stablecoin holding",
    );
    crate::shared::assert_position_slot(position, owner, position_nonce, stablecoin_program_id);

    // No PDA seed: the burn debits the user's own holding, authorized by the
    // transaction's witness set.
    let token_program_id = parameters.token_program_id;
    ChainedCall::new(
        token_program_id,
        vec![
            ProgramShardSelector::new(stablecoin_definition.account_id, token_program_id),
            ProgramShardSelector::new(user_stablecoin_holding.account_id, token_program_id),
        ],
        &token_core::Instruction::Burn {
            amount_to_burn: amount,
            kind: token_core::TokenKind::Fungible,
        },
    )
}

/// Apply half: shrink the position's normalized debt.
///
/// The delta is `⌊amount × FIXED_POINT_ONE / current_accumulator⌋`, rounded
/// **down** per §6.3: the borrower burned exactly `amount`, their debt shrinks by
/// at most that much, and the rounding remainder is fee credit for the protocol.
/// The accumulator is projected forward to the pinned `now` (§5.3).
///
/// # Panics
/// - the shard is not a valid [`Position`], or its identity differs from the declared one.
/// - the repayment exceeds the position's outstanding normalized debt.
#[must_use]
pub fn position_after_repay(pre_data: &ShardData, effect: &RepayDebt) -> ShardData {
    let position = crate::shared::position_of(pre_data, &effect.expected);
    let debt_delta = mul_div(effect.amount, FIXED_POINT_ONE, effect.fee.current_rate());
    let normalized_debt_amount = position
        .normalized_debt_amount
        .checked_sub(debt_delta)
        .expect("Repay amount exceeds outstanding normalized debt");
    ShardData::from(&Position {
        normalized_debt_amount,
        ..position
    })
}
