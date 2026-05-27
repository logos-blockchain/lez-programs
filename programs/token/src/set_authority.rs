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
    // The definition account must be authorized — this means the transaction
    // signer controls the definition account, which is how mint authority
    // is enforced in LEZ (account-level authorization).
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
                Some(_) => {
                    // Rotate to new authority, or revoke by setting to None
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
