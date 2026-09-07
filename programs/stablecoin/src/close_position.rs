use lee_core::{
    account::{Account, AccountWithMetadata, Data},
    program::{AccountPostState, ChainedCall, ProgramId},
};
use stablecoin_core::{
    compute_protocol_parameters_pda, verify_position_and_get_seed, Position, ProtocolParameters,
};
use token_core::TokenHolding;

/// Clear a fully-settled `position` (spec §10.9).
///
/// The position's data is zeroed but the account itself lingers: LEE's
/// `validate_execution` forbids a program from changing an account's
/// `program_owner` or `nonce`, so the PDA cannot be released and §10.9's
/// `Account::default()` post-state is not expressible. A consequence worth
/// knowing: the `(owner, position_nonce)` pair cannot be reused afterwards,
/// because `open_position` requires an uninitialized position account.
///
/// The vault is **not** closed either — the Token Program has no `CloseHolding`,
/// so it also lingers at `balance = 0` (§12, §14). It is passed read-only purely
/// so its emptiness can be asserted.
///
/// Allowed while the protocol is frozen: closing a settled position removes an
/// obligation and cannot worsen protocol health.
///
/// # Panics
/// - `owner` is not authorized.
/// - `position` is uninitialized, not owned by `stablecoin_program_id`, does not decode, or is not
///   at its `(owner, position_nonce)` PDA.
/// - `position.normalized_debt_amount` or `position.collateral_amount` is non-zero.
/// - `vault` does not match `Position.vault_account_id`, does not decode as a fungible
///   [`TokenHolding`], or still holds a balance.
/// - `protocol_parameters` is uninitialized, wrongly owned, not at its canonical PDA, or does not
///   decode.
pub fn close_position(
    owner: AccountWithMetadata,
    position: AccountWithMetadata,
    vault: AccountWithMetadata,
    protocol_parameters: AccountWithMetadata,
    stablecoin_program_id: ProgramId,
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

    // Read-only, but still pinned: a substituted global would be a silent
    // inconsistency even though nothing here branches on its contents.
    let _parameters = ProtocolParameters::try_from(&crate::checks::decode_global(
        &protocol_parameters,
        compute_protocol_parameters_pda(stablecoin_program_id),
        stablecoin_program_id,
        "ProtocolParameters",
    ))
    .expect("ProtocolParameters must decode");
    // `is_frozen` is deliberately not read: closing a settled position removes an
    // obligation and cannot worsen protocol health (§7).

    assert_eq!(
        position_data.normalized_debt_amount, 0,
        "Position still has outstanding debt"
    );
    assert_eq!(
        position_data.collateral_amount, 0,
        "Position still holds collateral"
    );

    assert_eq!(
        position_data.vault_account_id, vault.account_id,
        "Position vault_account_id does not match the vault account"
    );
    let vault_balance = match TokenHolding::try_from(&vault.account.data)
        .expect("Vault account must hold a valid TokenHolding")
    {
        TokenHolding::Fungible { balance, .. } => balance,
        TokenHolding::NftMaster { .. } | TokenHolding::NftPrintedCopy { .. } => {
            panic!("Vault must hold a fungible collateral balance")
        }
    };
    // The position's own accounting already reads zero; this catches the case
    // where the two disagree, which would strand tokens behind a released PDA.
    assert_eq!(vault_balance, 0, "Vault still holds a balance");

    let post_states = vec![
        AccountPostState::new(owner.account),
        // Clear the data only. The runtime forbids a program from changing an
        // account's `program_owner` or `nonce` (LEE `validate_execution` rules 3
        // and 4), so the PDA cannot actually be released — spec §10.9's
        // `Account::default()` is not expressible here. The account lingers
        // stablecoin-owned with empty data, like the vault does per §12.
        AccountPostState::new(Account {
            data: Data::default(),
            ..position.account
        }),
        AccountPostState::new(vault.account),
        AccountPostState::new(protocol_parameters.account),
    ];

    (post_states, vec![])
}
