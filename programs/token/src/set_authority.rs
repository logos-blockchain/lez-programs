use lez_authority::Ownable;
use nssa_core::{
    account::{AccountId, AccountWithMetadata, Data},
    program::{AccountPostState, ProgramId},
};
use token_core::TokenDefinition;

pub fn set_authority(
    definition_account: AccountWithMetadata,
    new_authority: Option<AccountId>,
    authority_accounts: Vec<AccountWithMetadata>,
    token_program_id: ProgramId,
) -> Vec<AccountPostState> {
    assert_eq!(
        definition_account.account.program_owner, token_program_id,
        "Token definition must be owned by token program"
    );

    let mut definition = TokenDefinition::try_from(&definition_account.account.data)
        .expect("Token Definition account must be valid");

    match &mut definition {
        TokenDefinition::Fungible { .. } => {
            // The current mint authority must authorize this transaction. As in
            // `mint`, the proof is either the definition account itself (empty
            // `authority_accounts`, self/PDA authority) or an explicit external
            // authority account (one entry), so a rotated authority can act.
            let authority = authority_accounts.first().unwrap_or(&definition_account);
            assert!(
                authority.is_authorized,
                "Mint authority must authorize the transaction"
            );
            let signer: [u8; 32] = authority
                .account_id
                .as_ref()
                .try_into()
                .expect("AccountId is always 32 bytes");

            match new_authority {
                Some(new) => {
                    let new_key: [u8; 32] = new
                        .as_ref()
                        .try_into()
                        .expect("AccountId is always 32 bytes");
                    assert!(
                        new_key != [0u8; 32],
                        "New mint authority must be a valid non-zero account ID"
                    );
                    definition
                        .transfer_ownership(signer, new_key)
                        .expect("SetAuthority failed");
                }
                None => {
                    definition
                        .renounce_ownership(signer)
                        .expect("SetAuthority failed");
                }
            }
        }
        TokenDefinition::NonFungible { .. } => {
            panic!("SetAuthority is not supported for Non-Fungible Tokens");
        }
    }

    let mut definition_post = definition_account.account;
    definition_post.data = Data::from(&definition);

    // Post-states match pre-state order/count: [definition, ...authority_accounts].
    let mut post_states = Vec::with_capacity(authority_accounts.len().saturating_add(1));
    post_states.push(AccountPostState::new(definition_post));
    for authority in authority_accounts {
        post_states.push(AccountPostState::new(authority.account));
    }
    post_states
}
