use lez_authority::Ownable;
use nssa_core::{
    account::{AccountId, AccountWithMetadata, Data},
    program::AccountPostState,
};
use token_core::TokenDefinition;

pub fn set_authority(
    definition_account: AccountWithMetadata,
    new_authority: Option<AccountId>,
) -> Vec<AccountPostState> {
    let mut definition = TokenDefinition::try_from(&definition_account.account.data)
        .expect("Token Definition account must be valid");

    match &mut definition {
        TokenDefinition::Fungible { .. } => {
            // The current mint authority must authorize this transaction: the
            // definition account must be authorized and its id must match the
            // stored authority.
            assert!(
                definition_account.is_authorized,
                "Mint authority must authorize the transaction"
            );
            let signer: [u8; 32] = definition_account
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

    vec![AccountPostState::new(definition_post)]
}
