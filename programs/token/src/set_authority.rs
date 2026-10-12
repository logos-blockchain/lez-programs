use lee_core::account::{AccountId, ShardData};
use token_core::TokenDefinition;

/// Definition-side effect: rotate or renounce the mint authority.
///
/// Like [`crate::mint::mint_supply`], the authority comparison belongs here
/// because the current authority is stored state while the claiming account is
/// plan-visible. Mutates only after every check passes, so a rejected call leaves
/// the prior authority intact.
///
/// # Panics
/// If the definition is non-fungible, the authority was already renounced, the
/// claiming account did not authorize, it is not the current authority, or
/// `new_authority` is the all-zero id.
#[must_use]
pub fn set_mint_authority(
    pre_data: &ShardData,
    new_authority: Option<AccountId>,
    authority_id: AccountId,
    authority_is_authorized: bool,
) -> ShardData {
    let mut definition =
        TokenDefinition::try_from(pre_data).expect("Token Definition account must be valid");

    match &mut definition {
        TokenDefinition::Fungible { authority, .. } => {
            let current = authority.expect("SetAuthority failed: authority already revoked");
            assert!(
                authority_is_authorized,
                "Mint authority must authorize the transaction"
            );
            assert_eq!(
                authority_id, current,
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

    ShardData::from(&definition)
}
