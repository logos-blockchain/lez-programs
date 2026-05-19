use nssa_core::{
    account::{Account, AccountWithMetadata, Data},
    program::{AccountPostState, ChainedCall, ProgramId},
};
use stablecoin_core::{verify_position_and_get_seed, verify_position_vault_and_get_seed, Position};
use token_core::TokenHolding;

/// Withdraw `amount` collateral tokens from `position`'s vault back to `destination`.
///
/// Decreases `Position.collateral_amount` by `amount` and emits a single chained
/// `Token::Transfer` from the vault to `destination`, authorized by the vault
/// PDA seed. The position post-state uses plain [`AccountPostState::new`] —
/// the initial PDA claim already happened in
/// [`crate::open_position::open_position`].
///
/// Until issues #95 / #96 / #97 land (redemption price, price feed, stability
/// fee accrual), this instruction hard-asserts `Position.debt_amount == 0`.
/// When those land, this guard is replaced by real fee accrual + a
/// collateralization-ratio check against the post-withdrawal collateral.
///
/// # Panics
/// - `owner` is not authorized.
/// - `position` is uninitialized, not owned by `stablecoin_program_id`, holds data that does not
///   decode as a [`Position`], or sits at an address that does not match
///   `compute_position_pda(stablecoin_program_id, owner, Position.collateral_definition_id)`.
/// - `vault` sits at an address that does not match
///   `compute_position_vault_pda(stablecoin_program_id, position_id)`, or holds a [`TokenHolding`]
///   whose `definition_id` does not match the position's collateral definition.
/// - `destination` is uninitialized, owned by a different Token Program than the vault, or holds a
///   [`TokenHolding`] whose `definition_id` does not match the position's collateral definition.
/// - `Position.debt_amount` is non-zero.
/// - `amount > Position.collateral_amount`.
pub fn withdraw_collateral(
    owner: AccountWithMetadata,
    position: AccountWithMetadata,
    vault: AccountWithMetadata,
    destination: AccountWithMetadata,
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
    // (owner, collateral_definition) PDA derivation. We do not use the seed
    // downstream — the position is already PDA-claimed.
    let _position_seed = verify_position_and_get_seed(
        &position,
        &owner,
        position_data.collateral_definition_id,
        stablecoin_program_id,
    );
    let vault_seed =
        verify_position_vault_and_get_seed(&vault, position.account_id, stablecoin_program_id);

    let vault_holding = TokenHolding::try_from(&vault.account.data)
        .expect("Vault account must hold a valid TokenHolding");
    assert_eq!(
        vault_holding.definition_id(),
        position_data.collateral_definition_id,
        "Vault token holding is not for the position's collateral definition"
    );

    let token_program_id = vault.account.program_owner;
    assert_ne!(
        destination.account,
        Account::default(),
        "Destination must be initialized"
    );
    assert_eq!(
        destination.account.program_owner, token_program_id,
        "Destination must be owned by the same Token Program as the vault"
    );
    let destination_holding = TokenHolding::try_from(&destination.account.data)
        .expect("Destination account must hold a valid TokenHolding");
    assert_eq!(
        destination_holding.definition_id(),
        position_data.collateral_definition_id,
        "Destination token definition does not match the position's collateral definition"
    );

    assert_eq!(
        position_data.debt_amount, 0,
        "withdraw_collateral with debt is not supported yet — stability fee accrual and collateralization check land with #97/#96"
    );
    let new_collateral = position_data
        .collateral_amount
        .checked_sub(amount)
        .expect("Withdrawal amount exceeds position collateral");

    let updated_position = Position {
        collateral_vault_id: position_data.collateral_vault_id,
        collateral_definition_id: position_data.collateral_definition_id,
        collateral_amount: new_collateral,
        debt_amount: position_data.debt_amount,
    };
    let mut position_post = position.account.clone();
    position_post.data = Data::from(&updated_position);

    let post_states = vec![
        AccountPostState::new(owner.account),
        AccountPostState::new(position_post),
        AccountPostState::new(vault.account.clone()),
        AccountPostState::new(destination.account.clone()),
    ];

    let mut vault_authorized = vault.clone();
    vault_authorized.is_authorized = true;
    let transfer_call = ChainedCall::new(
        token_program_id,
        vec![vault_authorized, destination],
        &token_core::Instruction::Transfer {
            amount_to_transfer: amount,
        },
    )
    .with_pda_seeds(vec![vault_seed]);

    (post_states, vec![transfer_call])
}
