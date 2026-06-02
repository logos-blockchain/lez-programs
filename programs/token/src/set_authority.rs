use lez_authority::AuthoritySlot;
use nssa_core::{
    account::{AccountWithMetadata, Data},
    program::AccountPostState,
};
use token_core::TokenDefinition;

pub fn set_authority(
    definition_account: AccountWithMetadata,
    authority_account: AccountWithMetadata,
    new_authority: Option<[u8; 32]>,
) -> Vec<AccountPostState> {
    let mut definition = TokenDefinition::try_from(&definition_account.account.data)
        .expect("Token Definition account must be valid");

    match &mut definition {
        TokenDefinition::Fungible { mint_authority, .. } => {
            assert!(
                authority_account.is_authorized,
                "Mint authority must sign the transaction"
            );
            let signer: [u8; 32] = authority_account
                .account_id
                .as_ref()
                .try_into()
                .expect("AccountId is always 32 bytes");
            let mut slot = AuthoritySlot(*mint_authority);
            slot.set(signer, new_authority)
                .expect("SetAuthority failed");
            *mint_authority = slot.0;
        }
        TokenDefinition::NonFungible { .. } => {
            panic!("SetAuthority is not supported for Non-Fungible Tokens");
        }
    }

    let mut definition_post = definition_account.account;
    definition_post.data = Data::from(&definition);

    vec![
        AccountPostState::new(definition_post),
        AccountPostState::new(authority_account.account),
    ]
}
