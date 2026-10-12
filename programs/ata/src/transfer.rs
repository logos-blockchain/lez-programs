use lee_core::{
    account::{AccountId, ShardData},
    program::{AccountMeta, PdaSeed},
};
use token_core::{TokenDescriptor, TokenHolding};

/// Plan half: authorize the owner and verify the sender ATA's address.
///
/// `descriptor.definition_id` is what the address derives from, so it has to be
/// an instruction field now — the plan can no longer read it out of the sender's
/// holding. The guards below pin it on both sides.
///
/// # Panics
/// If the owner is not authorized, or the ATA address does not derive from
/// `(owner, definition, token program)`.
pub fn plan_transfer(
    owner: &AccountMeta,
    sender_ata: &AccountMeta,
    ata_program_id: AccountId,
    token_program_id: AccountId,
    descriptor: &TokenDescriptor,
) -> PdaSeed {
    assert!(owner.is_authorized, "Owner authorization is missing");

    ata_core::verify_ata_and_get_seed(
        sender_ata,
        owner,
        token_program_id,
        descriptor.definition_id,
        ata_program_id,
    )
}

/// Guard on the recipient's shard: it must already hold the same asset.
///
/// This is an ATA-level contract, not something inherited from token: token's own
/// `deposit` would happily materialize an empty recipient from the descriptor.
/// Failing here instead means integrators get an ATA-level error rather than
/// having to reverse-engineer token and runtime semantics from a silently
/// created holding.
///
/// # Panics
/// If the recipient's shard is empty or holds a different asset.
pub fn check_recipient(pre_data: &ShardData, descriptor: &TokenDescriptor) {
    assert!(
        !pre_data.is_empty(),
        "Recipient token holding must be initialized"
    );
    let holding = TokenHolding::try_from(pre_data).expect("Recipient must hold a valid token");
    holding.assert_matches(descriptor);
}
