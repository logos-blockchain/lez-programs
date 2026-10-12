use lee_core::{
    account::AccountId,
    program::{AccountMeta, PdaSeed},
};
use token_core::TokenDescriptor;

/// Plan half: authorize the owner and verify the ATA's address.
///
/// The definition account's identity is pinned by `descriptor.definition_id`
/// matching it, asserted here, and by token's own `burn_holding` asserting the
/// holding against the same descriptor.
///
/// # Panics
/// If the owner is not authorized, the declared definition is not the definition
/// account supplied, or the ATA address does not derive as expected.
pub fn plan_burn(
    owner: &AccountMeta,
    holder_ata: &AccountMeta,
    token_definition: &AccountMeta,
    ata_program_id: AccountId,
    token_program_id: AccountId,
    descriptor: &TokenDescriptor,
) -> PdaSeed {
    assert!(owner.is_authorized, "Owner authorization is missing");
    assert_eq!(
        descriptor.definition_id, token_definition.account_id,
        "Holder ATA token definition does not match"
    );

    ata_core::verify_ata_and_get_seed(
        holder_ata,
        owner,
        token_program_id,
        descriptor.definition_id,
        ata_program_id,
    )
}
