use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, BalanceDiff, Data},
    program::{AccountStateDiff, ChainedCall},
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
#[allow(
    clippy::too_many_arguments,
    reason = "account inputs + program id + nonce + amount are all required; a param struct would obscure the host-call ABI"
)]
pub fn open_position(
    owner: AccountWithMetadata,
    position: AccountWithMetadata,
    vault: AccountWithMetadata,
    user_holding: AccountWithMetadata,
    token_definition: AccountWithMetadata,
    stablecoin_program_id: AccountId,
    position_nonce: u64,
    collateral_amount: u128,
) -> (Vec<AccountStateDiff>, Vec<ChainedCall>) {
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

    // Both calls assert the PDA derivation; only the vault seed is carried
    // further, as the authority for the chained InitializeAccount.
    verify_position_and_get_seed(&position, &owner, position_nonce, stablecoin_program_id);
    let vault_seed =
        verify_position_vault_and_get_seed(&vault, position.account_id, stablecoin_program_id);

    let vault_id = vault.account_id;
    let user_holding_id = user_holding.account_id;
    let token_definition_id = token_definition.account_id;

    // Writing the position data is itself the ownership claim on the position
    // PDA — v0.2.5 acquires ownership on data write, so no `Claim::Pda`.
    let position_data = Data::from(&Position {
        owner_account_id: owner.account_id,
        position_nonce,
        vault_account_id: vault_id,
        collateral_amount,
        normalized_debt_amount: 0,
        // TODO(#173): read from ctx clock once `open_position` is rebuilt with
        // the fee-aware flow. Setting 0 keeps #156 a pure refactor.
        opened_at: 0,
    });

    let state_diffs = vec![
        AccountStateDiff::unchanged(owner),
        AccountStateDiff::new(position, BalanceDiff::Add(0), position_data),
        AccountStateDiff::unchanged(vault),
        AccountStateDiff::unchanged(user_holding),
        AccountStateDiff::unchanged(token_definition),
    ];

    // Chained Token::InitializeAccount makes the vault a Token holding. The
    // Stablecoin program authorizes that write by passing the vault PDA seed —
    // the call ships account ids, so there is no pre-state flag to set.
    let initialize_call = ChainedCall::new(
        token_program_id,
        vec![token_definition_id, vault_id],
        &token_core::Instruction::InitializeAccount,
    )
    .with_pda_seeds(vec![vault_seed]);

    // After InitializeAccount the vault is a zero-balance Fungible holding for the
    // collateral definition. Token::Transfer only requires the sender to be authorized;
    // the recipient (vault) is already initialized by the call above, and since calls
    // name accounts by id the runtime resolves its post-InitializeAccount state — this
    // no longer has to hand-build a synthetic pre-state.
    let transfer_call = ChainedCall::new(
        token_program_id,
        vec![user_holding_id, vault_id],
        &token_core::Instruction::Transfer {
            amount_to_transfer: collateral_amount,
        },
    );

    (state_diffs, vec![initialize_call, transfer_call])
}
