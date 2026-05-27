use nssa_core::{
    account::{Account, AccountWithMetadata, Data},
    program::{AccountPostState, ChainedCall, ProgramId},
};
use stablecoin_core::{verify_position_and_get_seed, verify_position_vault_and_get_seed, Position};
use token_core::TokenHolding;

/// Deposit `amount` collateral tokens from `user_holding` into `position`'s vault.
///
/// Increases `Position.collateral_amount` by `amount` and emits a single chained
/// `Token::Transfer` from the user holding to the vault. No collateralization
/// check is required because debt is unchanged.
///
/// # Panics
/// - `owner` or `user_holding` is not authorized.
/// - `position` is uninitialized, not owned by `stablecoin_program_id`, holds data that does not
///   decode as a [`Position`], or sits at an address that does not match
///   `compute_position_pda(stablecoin_program_id, owner, Position.collateral_definition_id)`.
/// - `vault` is uninitialized, sits at an address that does not match
///   `compute_position_vault_pda(stablecoin_program_id, position_id)`, or holds a [`TokenHolding`]
///   whose `definition_id` does not match the position's collateral definition.
/// - `user_holding` is uninitialized, owned by a different Token Program than the vault, or holds a
///   [`TokenHolding`] whose `definition_id` does not match the position's collateral definition.
/// - `Position.collateral_amount + amount` overflows.
pub fn deposit_collateral(
    owner: AccountWithMetadata,
    position: AccountWithMetadata,
    vault: AccountWithMetadata,
    user_holding: AccountWithMetadata,
    stablecoin_program_id: ProgramId,
    amount: u128,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    assert!(owner.is_authorized, "Owner authorization is missing");
    assert!(
        user_holding.is_authorized,
        "User collateral holding authorization is missing"
    );
    assert_ne!(
        position.account,
        Account::default(),
        "Position account must be initialized"
    );
    assert_eq!(
        position.account.program_owner, stablecoin_program_id,
        "Position is not owned by this stablecoin program"
    );
    assert_ne!(
        vault.account,
        Account::default(),
        "Vault must be initialized"
    );
    assert_ne!(
        user_holding.account,
        Account::default(),
        "User collateral holding must be initialized"
    );

    let position_data = Position::try_from(&position.account.data)
        .expect("Position account must hold valid Position state");
    let _position_seed = verify_position_and_get_seed(
        &position,
        &owner,
        position_data.collateral_definition_id,
        stablecoin_program_id,
    );
    let _vault_seed =
        verify_position_vault_and_get_seed(&vault, position.account_id, stablecoin_program_id);
    assert_eq!(
        position_data.collateral_vault_id, vault.account_id,
        "Position collateral vault does not match provided vault"
    );

    let vault_holding = TokenHolding::try_from(&vault.account.data)
        .expect("Vault account must hold a valid TokenHolding");
    assert_eq!(
        vault_holding.definition_id(),
        position_data.collateral_definition_id,
        "Vault token holding is not for the position's collateral definition"
    );

    let token_program_id = vault.account.program_owner;
    assert_eq!(
        user_holding.account.program_owner, token_program_id,
        "User collateral holding must be owned by same Token Program as the vault"
    );
    let user_holding_data = TokenHolding::try_from(&user_holding.account.data)
        .expect("User collateral holding must hold a valid TokenHolding");
    assert_eq!(
        user_holding_data.definition_id(),
        position_data.collateral_definition_id,
        "User collateral holding does not match the position's collateral definition"
    );

    let new_collateral = position_data
        .collateral_amount
        .checked_add(amount)
        .expect("Deposit amount overflows position collateral");

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
        AccountPostState::new(user_holding.account.clone()),
    ];

    let transfer_call = ChainedCall::new(
        token_program_id,
        vec![user_holding, vault],
        &token_core::Instruction::Transfer {
            amount_to_transfer: amount,
        },
    );

    (post_states, vec![transfer_call])
}
