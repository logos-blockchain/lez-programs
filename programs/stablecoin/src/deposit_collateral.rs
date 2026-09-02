use lee_core::{
    account::{Account, AccountWithMetadata, Data},
    program::{AccountPostState, ChainedCall, ProgramId},
};
use stablecoin_core::{
    compute_protocol_parameters_pda, verify_position_and_get_seed, Position, ProtocolParameters,
};
use token_core::TokenHolding;

/// Deposit `amount` additional collateral tokens into an existing `position`'s vault.
///
/// Increases `Position.collateral_amount` by `amount` and emits a single chained
/// `Token::Transfer` from the user's authorized holding into the vault. No PDA
/// seed is attached: the sender is the user's own holding, which the transaction's
/// witness set authorizes.
///
/// Deliberately allowed while the protocol is frozen, and deliberately skips the
/// §6.2 collateralization check — a deposit can only improve the position, so
/// spec §7 keeps this path open in emergencies.
///
/// # Panics
/// - `owner` or `user_collateral_holding` is not authorized.
/// - `position` is uninitialized, not owned by `stablecoin_program_id`, does not decode as a
///   [`Position`], or sits at an address that does not match
///   `compute_position_pda(stablecoin_program_id, owner, Position.position_nonce)`.
/// - `Position.owner_account_id` does not match `owner`.
/// - `vault` does not match `Position.vault_account_id`, is uninitialized, does not decode as a
///   [`TokenHolding`], or holds a token other than the protocol's collateral definition.
/// - `protocol_parameters` is uninitialized, not owned by `stablecoin_program_id`, or does not
///   decode.
/// - `user_collateral_holding` is owned by a different Token Program than the vault, or its
///   `definition_id` is not `ProtocolParameters.collateral_definition_id`.
/// - `Position.collateral_amount + amount` overflows.
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
    // Binds the position to this owner. The seed is unused downstream — the
    // position was already PDA-claimed by `open_position`.
    let _position_seed = verify_position_and_get_seed(
        &position,
        &owner,
        position_data.position_nonce,
        stablecoin_program_id,
    );
    assert_eq!(
        position_data.owner_account_id, owner.account_id,
        "Position owner_account_id does not match the owner account"
    );
    assert_eq!(
        position_data.vault_account_id, vault.account_id,
        "Position vault_account_id does not match the vault account"
    );

    assert_ne!(
        protocol_parameters.account,
        Account::default(),
        "ProtocolParameters account must be initialized"
    );
    assert_eq!(
        protocol_parameters.account.program_owner, stablecoin_program_id,
        "ProtocolParameters account must be owned by the stablecoin program"
    );
    // Pin the address: ownership plus a successful decode would otherwise let
    // any stablecoin-owned account that decodes as ProtocolParameters stand in
    // for the global config and redirect the collateral-definition binding.
    assert_eq!(
        protocol_parameters.account_id,
        compute_protocol_parameters_pda(stablecoin_program_id),
        "ProtocolParameters account ID does not match expected PDA derivation"
    );
    let parameters = ProtocolParameters::try_from(&protocol_parameters.account.data)
        .expect("ProtocolParameters must decode");
    // `is_frozen` is deliberately not read: a deposit only improves the
    // position's collateralization, so spec §7 keeps it available when frozen.

    // Validate the vault before trusting its `program_owner` to route the chained
    // call. Without this the transfer would be aimed at whatever program owns the
    // account and left to fail downstream, and a vault holding some other token
    // would silently mis-bank the deposit.
    assert_ne!(
        vault.account,
        Account::default(),
        "Vault account must be initialized"
    );
    let vault_holding = TokenHolding::try_from(&vault.account.data)
        .expect("Vault account must hold a valid TokenHolding");
    assert_eq!(
        vault_holding.definition_id(),
        parameters.collateral_definition_id,
        "Vault holding does not match the protocol's collateral definition"
    );

    let token_program_id = vault.account.program_owner;
    assert_eq!(
        user_collateral_holding.account.program_owner, token_program_id,
        "User collateral holding must be owned by the same Token Program as the vault"
    );
    let user_holding = TokenHolding::try_from(&user_collateral_holding.account.data)
        .expect("User holding must be a valid TokenHolding");
    assert_eq!(
        user_holding.definition_id(),
        parameters.collateral_definition_id,
        "User collateral holding does not match the protocol's collateral definition"
    );

    let new_collateral = position_data
        .collateral_amount
        .checked_add(amount)
        .expect("Position collateral_amount overflow");

    let mut position_post = position.account.clone();
    position_post.data = Data::from(&Position {
        collateral_amount: new_collateral,
        ..position_data
    });

    let post_states = vec![
        AccountPostState::new(owner.account),
        AccountPostState::new(position_post),
        AccountPostState::new(vault.account.clone()),
        AccountPostState::new(user_collateral_holding.account.clone()),
        AccountPostState::new(protocol_parameters.account),
    ];

    // No PDA seed: the sender is the user's own holding, authorized by the
    // transaction's witness set. The receiving vault needs no authorization.
    let transfer_call = ChainedCall::new(
        token_program_id,
        vec![user_collateral_holding, vault],
        &token_core::Instruction::Transfer {
            amount_to_transfer: amount,
        },
    );

    (post_states, vec![transfer_call])
}
