use lee_core::account::{AccountId, ShardData};
use token_core::TokenDefinition;

/// Definition-side effect: add to the supply, gated on the stored mint authority.
///
/// The authority check cannot happen in the plan phase. The plan sees *which*
/// account claims authority and whether it authorized; the definition's shard
/// holds *who* the authority actually is. Only this apply has both, so the plan
/// passes its half in and the comparison happens here.
///
/// `authority_id` is the external authority account when the instruction supplied
/// one, and the definition account itself otherwise (self/PDA authority).
///
/// # Panics
/// If the definition is non-fungible, the authority was renounced, the claiming
/// account did not authorize, it is not the stored authority, or the supply would
/// overflow.
#[must_use]
pub fn mint_supply(
    pre_data: &ShardData,
    amount_to_mint: u128,
    authority_id: AccountId,
    authority_is_authorized: bool,
) -> ShardData {
    let mut definition =
        TokenDefinition::try_from(pre_data).expect("Token Definition account must be valid");

    let TokenDefinition::Fungible {
        total_supply,
        authority,
        ..
    } = &mut definition
    else {
        panic!("Cannot mint additional supply for Non-Fungible Tokens");
    };

    // `None` means the supply is permanently fixed (renounced) — minting is rejected.
    let mint_authority =
        authority.expect("Mint authority check failed: authority revoked, supply is fixed");
    assert!(
        authority_is_authorized,
        "Mint authority must authorize the transaction"
    );
    assert_eq!(
        authority_id, mint_authority,
        "Mint authority check failed: signer is not the current authority"
    );

    *total_supply = total_supply
        .checked_add(amount_to_mint)
        .expect("Total supply overflow");

    ShardData::from(&definition)
}
