use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, BalanceDiff, Data},
    program::{AccountStateDiff, ChainedCall},
};
use stablecoin_core::{
    compute_protocol_parameters_pda, verify_position_and_get_seed,
    verify_position_vault_and_get_seed, Position, ProtocolParameters,
};
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
/// - `owner` or `user_collateral_holding` is not authorized.
/// - `position` or `vault` is already initialized.
/// - `position.account_id` / `vault.account_id` do not match their PDA derivations.
/// - `protocol_parameters` is uninitialized, not owned by `stablecoin_program_id`, not at
///   `compute_protocol_parameters_pda(stablecoin_program_id)`, or does not decode.
/// - `protocol_parameters.is_frozen` is set.
/// - `collateral_definition.account_id` does not match
///   `protocol_parameters.collateral_definition_id`.
/// - `clock` is not the system `CLOCK_01` account, or is uninitialized.
/// - `user_collateral_holding` cannot be decoded as a [`TokenHolding`].
/// - `user_collateral_holding`'s definition does not match `collateral_definition`.
/// - `collateral_definition.program_owner` does not match `user_collateral_holding.program_owner`.
#[allow(
    clippy::too_many_arguments,
    reason = "account inputs + program id + nonce + amount are all required; a param struct would obscure the host-call ABI"
)]
pub fn open_position(
    owner: AccountWithMetadata,
    position: AccountWithMetadata,
    vault: AccountWithMetadata,
    user_collateral_holding: AccountWithMetadata,
    collateral_definition: AccountWithMetadata,
    protocol_parameters: AccountWithMetadata,
    clock: AccountWithMetadata,
    stablecoin_program_id: AccountId,
    position_nonce: u64,
    initial_collateral_amount: u128,
) -> (Vec<AccountStateDiff>, Vec<ChainedCall>) {
    assert!(owner.is_authorized, "Owner authorization is missing");
    assert!(
        user_collateral_holding.is_authorized,
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

    assert_ne!(
        protocol_parameters.account,
        Account::default(),
        "ProtocolParameters account must be initialized"
    );
    assert_eq!(
        protocol_parameters.account.program_owner, stablecoin_program_id,
        "ProtocolParameters account must be owned by the stablecoin program"
    );
    // Ownership and a successful decode are not enough: without pinning the
    // address, any stablecoin-owned account that happens to decode as
    // ProtocolParameters could stand in for the global config and bypass both
    // the freeze flag and the collateral binding below.
    assert_eq!(
        protocol_parameters.account_id,
        compute_protocol_parameters_pda(stablecoin_program_id),
        "ProtocolParameters account ID does not match expected PDA derivation"
    );
    let parameters = ProtocolParameters::try_from(&protocol_parameters.account.data)
        .expect("ProtocolParameters must decode");
    assert!(!parameters.is_frozen, "Protocol is frozen");
    assert_eq!(
        collateral_definition.account_id, parameters.collateral_definition_id,
        "Collateral definition does not match the one bound at initialize_program"
    );

    let now = crate::accrue_stability_fee::read_clock(&clock);

    let user_collateral_holding_definition_id =
        TokenHolding::try_from(&user_collateral_holding.account.data)
            .expect("User collateral holding must be a valid TokenHolding")
            .definition_id();
    assert_eq!(
        user_collateral_holding_definition_id, collateral_definition.account_id,
        "User collateral holding does not match the collateral definition"
    );
    let token_program_id = user_collateral_holding.account.program_owner;
    assert_eq!(
        collateral_definition.account.program_owner, token_program_id,
        "Collateral definition is not owned by the user collateral holding's Token Program"
    );

    // Both calls assert the PDA derivation; only the vault seed is carried
    // further, as the authority for the chained InitializeAccount.
    verify_position_and_get_seed(&position, &owner, position_nonce, stablecoin_program_id);
    let vault_seed =
        verify_position_vault_and_get_seed(&vault, position.account_id, stablecoin_program_id);

    let vault_id = vault.account_id;
    let user_collateral_holding_id = user_collateral_holding.account_id;
    let collateral_definition_id = collateral_definition.account_id;

    // Writing the position data is itself the ownership claim on the position
    // PDA — v0.2.5 acquires ownership on data write, so no `Claim::Pda`.
    let position_data = Data::from(&Position {
        owner_account_id: owner.account_id,
        position_nonce,
        vault_account_id: vault_id,
        collateral_amount: initial_collateral_amount,
        normalized_debt_amount: 0,
        opened_at: now,
    });

    let state_diffs = vec![
        AccountStateDiff::unchanged(owner),
        AccountStateDiff::new(position, BalanceDiff::Add(0), position_data),
        AccountStateDiff::unchanged(vault),
        AccountStateDiff::unchanged(user_collateral_holding),
        AccountStateDiff::unchanged(collateral_definition),
        AccountStateDiff::unchanged(protocol_parameters),
        AccountStateDiff::unchanged(clock),
    ];

    // Chained Token::InitializeAccount makes the vault a Token holding. The
    // Stablecoin program authorizes that write by passing the vault PDA seed —
    // the call ships account ids, so there is no pre-state flag to set.
    let initialize_call = ChainedCall::new(
        token_program_id,
        vec![collateral_definition_id, vault_id],
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
        vec![user_collateral_holding_id, vault_id],
        &token_core::Instruction::Transfer {
            amount_to_transfer: initial_collateral_amount,
        },
    );

    (state_diffs, vec![initialize_call, transfer_call])
}
