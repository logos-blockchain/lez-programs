use nssa_core::{
    account::{AccountId, AccountWithMetadata, Data},
    program::{AccountPostState, ProgramId},
};
use token_core::TokenDefinition;

/// Rotate or revoke the mint authority under **self/PDA authority**: the definition
/// account itself is the current authority and proves it by being authorized in this
/// transaction (a signer, or a PDA authorized under its seeds).
pub fn set_authority(
    definition_account: AccountWithMetadata,
    new_authority: Option<AccountId>,
    token_program_id: ProgramId,
) -> Vec<AccountPostState> {
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
    token_program_id: ProgramId,
) -> Vec<AccountPostState> {
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
    token_program_id: ProgramId,
) -> Vec<AccountPostState> {
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

    let mut definition_post = definition_account.account;
    definition_post.data = Data::from(&definition);

    // Post-states must match pre-state order and count: [definition] for self
    // authority, plus the read-only authority account when external.
    let mut post_states = Vec::with_capacity(2);
    post_states.push(AccountPostState::new(definition_post));
    if let Some(authority) = authority_account {
        post_states.push(AccountPostState::new(authority.account));
    }
    post_states
}
