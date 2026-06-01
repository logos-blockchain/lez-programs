use nssa_core::{
    account::{AccountWithMetadata, Data},
    program::AccountPostState,
};
use token_core::TokenDefinition;

#[must_use]
pub fn set_authority(
    definition_account: AccountWithMetadata,
    new_authority: Option<[u8; 32]>,
) -> Vec<AccountPostState> {
    assert!(
        definition_account.is_authorized,
        "Definition account authorization is missing; only the mint authority can call SetAuthority"
    );

    let mut definition = TokenDefinition::try_from(&definition_account.account.data)
        .expect("Token Definition account must be valid");

    match &mut definition {
        TokenDefinition::Fungible { mint_authority, .. } => {
            match mint_authority {
                None => {
                    panic!("Mint authority already revoked; supply is permanently fixed");
                }
                Some(authority_key) => {
                    // Validate caller matches the stored mint authority key
                    assert_eq!(
                        definition_account.account_id.as_ref(),
                        authority_key.as_ref(),
                        "Signer does not match the stored mint authority"
                    );
                    *mint_authority = new_authority;
                }
            }
        }
        TokenDefinition::NonFungible { .. } => {
            panic!("SetAuthority is not supported for Non-Fungible Tokens");
        }
    }

    let mut definition_post = definition_account.account;
    definition_post.data = Data::from(&definition);

    vec![AccountPostState::new(definition_post)]
}
