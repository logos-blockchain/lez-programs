use nssa_core::{
    account::{Account, AccountWithMetadata, Data},
    program::{AccountPostState, ChainedCall, ProgramId},
};
use stablecoin_core::{verify_position_and_get_seed, verify_position_vault_and_get_seed, Position};
use token_core::TokenHolding;

use crate::shared::{
    read_clock_timestamp, read_protocol_parameters, read_redemption_price_state,
    read_stability_fee_accumulator,
};

/// Withdraw `amount` collateral tokens from `position`'s vault back to `destination`.
///
/// Decreases `Position.collateral_amount` by `amount` and emits a single chained
/// `Token::Transfer` from the vault to `destination`, authorized by the vault
/// PDA seed. The position post-state uses plain [`AccountPostState::new`] —
/// the initial PDA claim already happened in
/// [`crate::open_position::open_position`].
///
/// # Panics
/// - `owner` is not authorized.
/// - `position` is uninitialized, not owned by `stablecoin_program_id`, holds data that does not
///   decode as a [`Position`], or sits at an address that does not match
///   `compute_position_pda(stablecoin_program_id, owner, Position.position_nonce)`.
/// - `vault` sits at an address that does not match
///   `compute_position_vault_pda(stablecoin_program_id, position_id)`, or holds a [`TokenHolding`]
///   whose `definition_id` does not match the protocol collateral definition.
/// - `destination` is uninitialized, owned by a different Token Program than the vault, or holds a
///   [`TokenHolding`] whose `definition_id` does not match the protocol collateral definition.
/// - `amount > Position.collateral_amount`.
/// - the post-withdrawal position would be undercollateralized.
#[expect(
    clippy::too_many_arguments,
    reason = "instruction surface passes explicit position, vault, fee, redemption, and protocol accounts"
)]
pub fn withdraw_collateral(
    owner: AccountWithMetadata,
    position: AccountWithMetadata,
    vault: AccountWithMetadata,
    destination: AccountWithMetadata,
    stability_fee_accumulator: AccountWithMetadata,
    redemption_price_state: AccountWithMetadata,
    protocol_parameters: AccountWithMetadata,
    clock: AccountWithMetadata,
    stablecoin_program_id: ProgramId,
    amount: u128,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    assert!(owner.is_authorized, "Owner authorization is missing");
    let params = read_protocol_parameters(&protocol_parameters, stablecoin_program_id);
    assert!(
        !params.is_frozen,
        "Protocol is frozen; collateral withdrawal is disabled"
    );
    let accumulator =
        read_stability_fee_accumulator(&stability_fee_accumulator, stablecoin_program_id);
    let redemption_state =
        read_redemption_price_state(&redemption_price_state, stablecoin_program_id);
    let now = read_clock_timestamp(&clock);
    let current_accumulator = stablecoin_core::current_accumulated_rate(&accumulator, &params, now);
    let current_redemption_price =
        stablecoin_core::current_redemption_price(&redemption_state, now);

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
    assert_eq!(
        position_data.owner_account_id, owner.account_id,
        "Position owner does not match signer"
    );
    // `verify_position_and_get_seed` asserts the position address matches the
    // (owner, position_nonce) PDA derivation. We do not use the seed
    // downstream — the position is already PDA-claimed.
    let _position_seed = verify_position_and_get_seed(
        &position,
        &owner,
        position_data.position_nonce,
        stablecoin_program_id,
    );
    assert_eq!(
        vault.account_id, position_data.vault_account_id,
        "Vault account does not match position vault"
    );
    let vault_seed =
        verify_position_vault_and_get_seed(&vault, position.account_id, stablecoin_program_id);

    let vault_holding = TokenHolding::try_from(&vault.account.data)
        .expect("Vault account must hold a valid TokenHolding");
    assert_eq!(
        vault_holding.definition_id(),
        params.collateral_definition_id,
        "Vault token holding does not match protocol collateral definition"
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
        params.collateral_definition_id,
        "Destination token definition does not match protocol collateral definition"
    );

    let new_collateral = position_data
        .collateral_amount
        .checked_sub(amount)
        .expect("Withdrawal amount exceeds position collateral");
    assert!(
        stablecoin_core::is_collateralized(
            new_collateral,
            position_data.normalized_debt_amount,
            current_accumulator,
            current_redemption_price,
            params.minimum_collateralization_ratio,
        ),
        "Position would be undercollateralized after withdrawal"
    );

    let updated_position = Position {
        owner_account_id: position_data.owner_account_id,
        position_nonce: position_data.position_nonce,
        vault_account_id: position_data.vault_account_id,
        collateral_amount: new_collateral,
        normalized_debt_amount: position_data.normalized_debt_amount,
        opened_at: position_data.opened_at,
    };
    let mut position_post = position.account.clone();
    position_post.data = Data::from(&updated_position);

    let post_states = vec![
        AccountPostState::new(owner.account),
        AccountPostState::new(position_post),
        AccountPostState::new(vault.account.clone()),
        AccountPostState::new(destination.account.clone()),
        AccountPostState::new(stability_fee_accumulator.account),
        AccountPostState::new(redemption_price_state.account),
        AccountPostState::new(protocol_parameters.account),
        AccountPostState::new(clock.account.clone()),
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
