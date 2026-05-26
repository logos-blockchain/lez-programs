use nssa_core::{
    account::{Account, AccountWithMetadata, Data},
    program::{AccountPostState, ChainedCall, ProgramId},
};
use stablecoin_core::{verify_position_and_get_seed, Position};
use token_core::TokenHolding;

/// Repay `amount` of outstanding stablecoin debt against an existing position.
///
/// Burns `amount` stablecoins from `user_stablecoin_holding` via a chained
/// `Token::Burn` and decreases `Position.debt_amount` by the same amount. The
/// position post-state uses plain [`AccountPostState::new`] — the PDA was
/// already claimed at `open_position` time.
///
/// Until issue #97 (stability fee accrual) lands, the fee-accrual step is a
/// no-op (every position structurally has `debt_amount = 0` today because
/// `generate_debt` is unimplemented; "fees-accrued" is therefore vacuously
/// true). A `// TODO(#97)` comment marks where the accrual code will plug in
/// — right before the `checked_sub` below.
///
/// Until issue #91 (`generate_debt`) records the stablecoin definition into
/// `Position`, this instruction cannot validate that `stablecoin_definition`
/// is the correct one for the position's debt. The caller is trusted.
///
/// # Panics
/// - `owner` is not authorized.
/// - `position` is uninitialized, not owned by `stablecoin_program_id`, holds data that does not
///   decode as a [`Position`], or sits at an address that does not match
///   `compute_position_pda(stablecoin_program_id, owner, Position.collateral_definition_id)`.
/// - `user_stablecoin_holding` is not authorized, is uninitialized, is owned by a different Token
///   Program than `stablecoin_definition`, or holds a [`TokenHolding`] whose `definition_id` does
///   not match `stablecoin_definition.account_id`.
/// - `stablecoin_definition` is uninitialized.
/// - `amount > Position.debt_amount`.
pub fn repay_debt(
    owner: AccountWithMetadata,
    position: AccountWithMetadata,
    stablecoin_definition: AccountWithMetadata,
    user_stablecoin_holding: AccountWithMetadata,
    stablecoin_program_id: ProgramId,
    amount: u128,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    assert!(owner.is_authorized, "Owner authorization is missing");
    assert_ne!(
        position.account,
        Account::default(),
        "Position account must be initialized"
    );
    assert_eq!(
        position.account.program_owner, stablecoin_program_id,
        "Position is not owned by this stablecoin program"
    );

    let position_data = Position::try_from(&position.account.data)
        .expect("Position account must hold valid Position state");
    // `verify_position_and_get_seed` asserts the position address matches the
    // (owner, collateral_definition) PDA derivation. The returned seed is
    // dropped — the position is already PDA-claimed.
    let _position_seed = verify_position_and_get_seed(
        &position,
        &owner,
        position_data.collateral_definition_id,
        stablecoin_program_id,
    );

    assert!(
        user_stablecoin_holding.is_authorized,
        "User stablecoin holding authorization is missing"
    );
    assert_ne!(
        user_stablecoin_holding.account,
        Account::default(),
        "User stablecoin holding must be initialized"
    );
    assert_ne!(
        stablecoin_definition.account,
        Account::default(),
        "Stablecoin definition account must be initialized"
    );
    assert_eq!(
        user_stablecoin_holding.account.program_owner, stablecoin_definition.account.program_owner,
        "Stablecoin holding and definition must be owned by the same Token Program"
    );
    let user_holding_data = TokenHolding::try_from(&user_stablecoin_holding.account.data)
        .expect("User stablecoin holding must hold a valid TokenHolding");
    assert_eq!(
        user_holding_data.definition_id(),
        stablecoin_definition.account_id,
        "Stablecoin holding does not match the provided stablecoin definition"
    );

    // TODO(#97): accrue stability fees onto position_data.debt_amount here, before
    // the checked_sub below. Today every position has debt_amount = 0 (no
    // generate_debt yet), so the precondition is trivially met.
    let new_debt = position_data
        .debt_amount
        .checked_sub(amount)
        .expect("Repay amount exceeds outstanding debt");

    let updated_position = Position {
        collateral_vault_id: position_data.collateral_vault_id,
        collateral_definition_id: position_data.collateral_definition_id,
        collateral_amount: position_data.collateral_amount,
        debt_amount: new_debt,
    };
    let mut position_post = position.account.clone();
    position_post.data = Data::from(&updated_position);

    let post_states = vec![
        AccountPostState::new(owner.account),
        AccountPostState::new(position_post),
        AccountPostState::new(stablecoin_definition.account.clone()),
        AccountPostState::new(user_stablecoin_holding.account.clone()),
    ];

    let token_program_id = user_stablecoin_holding.account.program_owner;
    let burn_call = ChainedCall::new(
        token_program_id,
        vec![stablecoin_definition, user_stablecoin_holding],
        &token_core::Instruction::Burn {
            amount_to_burn: amount,
        },
    );

    (post_states, vec![burn_call])
}
