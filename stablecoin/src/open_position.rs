use nssa_core::{
    account::{Account, AccountWithMetadata, Data},
    program::{AccountPostState, ChainedCall, Claim, ProgramId},
};
use stablecoin_core::{verify_position_and_get_seed, verify_position_vault_and_get_seed, Position};
use token_core::TokenHolding;

/// Open a new collateral-only position for `owner`.
///
/// This claims the [`Position`] PDA, issues two chained token-program calls under the
/// stablecoin's PDA authority, and stores `collateral_amount` with `debt_amount = 0`:
/// 1. `InitializeAccount` materializes the vault token holding for the collateral.
/// 2. `Transfer` moves `collateral_amount` collateral tokens from the user's holding into the
///    freshly initialized vault.
///
/// `debt_amount` is deferred to a future `generate_debt` instruction and is intentionally
/// not parameterized here.
///
/// # Panics
/// - `owner` or `user_holding` is not authorized.
/// - `position` or `vault` is already initialized.
/// - `position.account_id` / `vault.account_id` do not match their PDA derivations.
/// - `user_holding` cannot be decoded as a [`TokenHolding`].
/// - `user_holding`'s definition does not match `token_definition`.
/// - `token_definition.program_owner` does not match `user_holding.program_owner`.
pub fn open_position(
    owner: AccountWithMetadata,
    position: AccountWithMetadata,
    vault: AccountWithMetadata,
    user_holding: AccountWithMetadata,
    token_definition: AccountWithMetadata,
    stablecoin_program_id: ProgramId,
    collateral_amount: u128,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    assert!(owner.is_authorized, "Owner authorization is missing");
    assert!(
        user_holding.is_authorized,
        "User collateral holding authorization is missing"
    );
    assert_eq!(
        position.account,
        Account::default(),
        "Position account must be uninitialized"
    );
    assert_eq!(
        vault.account,
        Account::default(),
        "Position vault account must be uninitialized"
    );

    let user_holding_definition_id = TokenHolding::try_from(&user_holding.account.data)
        .expect("User holding must be a valid Token Holding")
        .definition_id();
    assert_eq!(
        user_holding_definition_id, token_definition.account_id,
        "User collateral holding does not match the provided token definition"
    );
    let token_program_id = user_holding.account.program_owner;
    assert_eq!(
        token_definition.account.program_owner, token_program_id,
        "Collateral token definition is not owned by the user holding's Token Program"
    );

    let position_seed = verify_position_and_get_seed(
        &position,
        &owner,
        token_definition.account_id,
        stablecoin_program_id,
    );
    let vault_seed =
        verify_position_vault_and_get_seed(&vault, position.account_id, stablecoin_program_id);

    let mut position_post = position.account;
    position_post.program_owner = stablecoin_program_id;
    position_post.data = Data::from(&Position {
        collateral_vault_id: vault.account_id,
        collateral_definition_id: token_definition.account_id,
        collateral_amount,
        debt_amount: 0,
    });

    let post_states = vec![
        AccountPostState::new(owner.account),
        AccountPostState::new_claimed(position_post, Claim::Pda(position_seed)),
        AccountPostState::new(vault.account.clone()),
        AccountPostState::new(user_holding.account.clone()),
        AccountPostState::new(token_definition.account.clone()),
    ];

    // Chained Token::InitializeAccount owns the vault as a Token holding. The Stablecoin
    // program only authorizes that claim by passing the vault PDA seed to the chained call.
    let mut vault_authorized = vault.clone();
    vault_authorized.is_authorized = true;
    let initialize_call = ChainedCall::new(
        token_program_id,
        vec![token_definition.clone(), vault_authorized],
        &token_core::Instruction::InitializeAccount,
    )
    .with_pda_seeds(vec![vault_seed]);

    // After InitializeAccount the vault is a zero-balance Fungible holding for the
    // collateral definition. Token::Transfer only requires the sender to be authorized; the
    // recipient (vault) is already initialized, so no second PDA claim is needed here.
    let post_init_vault = AccountWithMetadata {
        account: Account {
            program_owner: token_program_id,
            balance: 0,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: token_definition.account_id,
                balance: 0,
            }),
            nonce: vault.account.nonce,
        },
        is_authorized: false,
        account_id: vault.account_id,
    };
    let transfer_call = ChainedCall::new(
        token_program_id,
        vec![user_holding, post_init_vault],
        &token_core::Instruction::Transfer {
            amount_to_transfer: collateral_amount,
        },
    );

    (post_states, vec![initialize_call, transfer_call])
}
