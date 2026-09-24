use nssa_core::{
    account::{Account, AccountWithMetadata, Data},
    program::{AccountPostState, ChainedCall, ProgramId},
};
use stablecoin_core::{verify_position_and_get_seed, verify_position_vault_and_get_seed, Position};
use token_core::TokenHolding;

/// Deposit `amount` collateral tokens from `user_collateral_holding` into `position`'s vault.
///
/// Increases `Position.collateral_amount` by `amount` and emits a single chained
/// `Token::Transfer` from the user holding to the vault, authorized by the user
/// holding PDA. The position post-state uses plain [`AccountPostState::new`] —
/// the initial PDA claim already happened in
/// [`crate::open_position::open_position`].
///
/// # Panics
/// - `owner` or `user_collateral_holding` is not authorized.
/// - `position` is uninitialized, not owned by `stablecoin_program_id`, holds data that does not
///   decode as a [`Position`], or sits at an address that does not match
///   `compute_position_pda(stablecoin_program_id, owner, Position.collateral_definition_id)`.
/// - `vault` sits at an address that does not match
///   `compute_position_vault_pda(stablecoin_program_id, position_id)`, or holds a [`TokenHolding`]
///   whose `definition_id` does not match the position's collateral definition.
/// - `user_collateral_holding` is uninitialized, owned by a different Token Program than the vault,
///   or holds a [`TokenHolding`] whose `definition_id` does not match the position's collateral
///   definition.
/// - `amount` is zero or exceeds `user_collateral_holding` balance.
pub fn deposit_collateral(
    owner: AccountWithMetadata,
    position: AccountWithMetadata,
    vault: AccountWithMetadata,
    user_collateral_holding: AccountWithMetadata,
    protocol_parameters: AccountWithMetadata,
    stablecoin_program_id: ProgramId,
    amount: u128,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    assert!(owner.is_authorized, "Owner authorization is missing");
    assert!(
        user_collateral_holding.is_authorized,
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

    let position_data = Position::try_from(&position.account.data)
        .expect("Position account must hold valid Position state");

    let _position_seed = verify_position_and_get_seed(
        &position,
        &owner,
        position_data.collateral_definition_id,
        stablecoin_program_id,
    );

    let vault_seed =
        verify_position_vault_and_get_seed(&vault, position.account_id, stablecoin_program_id);

    let vault_holding = TokenHolding::try_from(&vault.account.data)
        .expect("Vault account must hold valid TokenHolding state");
    assert_eq!(
        vault_holding.definition_id(),
        position_data.collateral_definition_id,
        "Vault holding definition does not match position collateral definition"
    );

    let user_holding = TokenHolding::try_from(&user_collateral_holding.account.data)
        .expect("User collateral holding must hold valid TokenHolding state");
    assert_eq!(
        user_holding.definition_id(),
        position_data.collateral_definition_id,
        "User collateral holding definition does not match position collateral definition"
    );
    assert_eq!(
        user_collateral_holding.account.program_owner, vault.account.program_owner,
        "User collateral holding is not owned by the same Token Program as the vault"
    );

    assert!(amount > 0, "Deposit amount must be non-zero");

    let new_collateral_amount = position_data
        .collateral_amount
        .checked_add(amount)
        .expect("Collateral amount overflow");

    let mut position_post = position.account.clone();
    position_post.data = Data::from(&Position {
        collateral_amount: new_collateral_amount,
        ..position_data
    });

    let post_states = vec![
        AccountPostState::new(owner.account),
        AccountPostState::new(position_post),
        AccountPostState::new(vault.account.clone()),
        AccountPostState::new(user_collateral_holding.account.clone()),
        AccountPostState::new(protocol_parameters.account),
    ];

    let token_program_id = vault.account.program_owner;
    let mut vault_authorized = vault.clone();
    vault_authorized.is_authorized = true;

    let transfer = ChainedCall::new(
        token_program_id,
        vec![user_collateral_holding, vault_authorized],
        &token_core::Instruction::Transfer {
            amount_to_transfer: amount,
        },
    )
    .with_pda_seeds(vec![vault_seed]);

    (post_states, vec![transfer])
}
