use lee_core::{
    account::{AccountId, AccountWithMetadata, BalanceDiff, Data},
    program::AccountStateDiff,
};
use token_core::TokenDefinition;

/// Rotate or revoke the mint authority under **self/PDA authority**: the definition
/// account itself is the current authority and proves it by being authorized in this
/// transaction (a signer, or a PDA authorized under its seeds).
pub fn set_authority(
    definition_account: AccountWithMetadata,
    new_authority: Option<AccountId>,
    token_program_id: AccountId,
) -> Vec<AccountStateDiff> {
    set_authority_inner(definition_account, None, new_authority, token_program_id)
}

/// Rotate or revoke the mint authority under an **external authority**: a distinct
/// account (the account the authority was previously rotated to) proves authority by
/// signing, so a rotated authority can rotate or revoke again without the definition's
/// key. The definition account is mutated but does not authorize the change.
pub fn set_authority_with_authority(
    definition_account: AccountWithMetadata,
    authority_account: AccountWithMetadata,
    new_authority: Option<AccountId>,
    token_program_id: AccountId,
) -> Vec<AccountStateDiff> {
    set_authority_inner(
        definition_account,
        Some(authority_account),
        new_authority,
        token_program_id,
    )
}

/// Shared rotation/revocation core for both authority modes. `authority_account` is
/// the external authority when `Some`; when `None` the definition account itself is
/// treated as the authority (self/PDA authority). Only mutates state after all checks
/// pass, so a rejected call leaves the prior authority intact. Post-state order mirrors
/// the pre-state account order for each mode.
fn set_authority_inner(
    definition_account: AccountWithMetadata,
    authority_account: Option<AccountWithMetadata>,
    new_authority: Option<AccountId>,
    token_program_id: AccountId,
) -> Vec<AccountStateDiff> {
    assert_eq!(
        definition_account.account.program_owner, token_program_id,
        "Token definition must be owned by token program"
    );

    let mut definition = TokenDefinition::try_from(&definition_account.account.data)
        .expect("Token Definition account must be valid");

    match &mut definition {
        TokenDefinition::Fungible { authority, .. } => {
            // The account that proves authority must be authorized AND its id must
            // match the stored authority. That account is the explicit external
            // authority when present, otherwise the definition account itself.
            // `None` means the authority was renounced and can no longer be set.
            let current = authority.expect("SetAuthority failed: authority already revoked");
            let authority_ref = authority_account.as_ref().unwrap_or(&definition_account);
            assert!(
                authority_ref.is_authorized,
                "Mint authority must authorize the transaction"
            );
            assert_eq!(
                authority_ref.account_id, current,
                "SetAuthority failed: signer is not the current authority"
            );

            if let Some(new) = &new_authority {
                assert!(
                    new.value() != &[0u8; 32],
                    "New mint authority must be a valid non-zero account ID"
                );
            }
            // Rotate to the new authority, or renounce with `None`.
            *authority = new_authority;
        }
        TokenDefinition::NonFungible { .. } => {
            panic!("SetAuthority is not supported for Non-Fungible Tokens");
        }
    }

    // Diffs must match pre-state order and count: [definition] for self
    // authority, plus the read-only authority account when external.
    let mut state_diffs = Vec::with_capacity(2);
    state_diffs.push(AccountStateDiff::new(
        definition_account,
        BalanceDiff::Add(0),
        Data::from(&definition),
    ));
    if let Some(authority) = authority_account {
        state_diffs.push(AccountStateDiff::unchanged(authority));
    }
    state_diffs
}
