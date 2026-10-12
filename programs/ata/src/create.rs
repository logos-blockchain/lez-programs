use ata_core::AtaContents;
use lee_core::{
    account::{AccountId, ShardData},
    program::{AccountMeta, PdaSeed},
};
use token_core::{TokenDefinition, TokenDescriptor, TokenKind};

/// Plan half: verify the ATA address and report whether a chained initialize is
/// needed.
///
/// `contents` decides that, and arrives as a caller claim — so the guard that
/// pins it (`check_contents`) must be emitted whatever branch is taken. Taking
/// `contents` on the caller's word would be a hole, not a shortcut: `Empty`
/// claimed over a funded ATA would hand the token program a seed-authorized
/// `InitializeAccount` and zeroize the holding, with no owner signature anywhere.
/// Token's own `initialize_holding` cannot stop that, because the PDA seed is
/// exactly what makes the ATA authorized in the child call.
///
/// # Panics
/// If the ATA address does not derive from `(owner, definition, token program)`,
/// or a squatted ATA is being replaced without the owner's authorization.
pub fn plan_create(
    owner: &AccountMeta,
    token_definition: &AccountMeta,
    ata_account: &AccountMeta,
    ata_program_id: AccountId,
    token_program_id: AccountId,
    contents: AtaContents,
) -> (PdaSeed, bool) {
    let seed = ata_core::verify_ata_and_get_seed(
        ata_account,
        owner,
        token_program_id,
        token_definition.account_id,
        ata_program_id,
    );

    let needs_initialize = match contents {
        AtaContents::Empty => true,
        AtaContents::Intended => false,
        AtaContents::Squatted => {
            // Replacing someone else's bytes at this address is the one branch
            // that destroys value, so it needs the owner to say so.
            assert!(owner.is_authorized, "Owner authorization is missing");
            true
        }
    };

    (seed, needs_initialize)
}

/// Guard on the ATA's shard: the declared `contents` must be what is really there.
///
/// Reads a Token Program shard and writes nothing — this program owns no shard,
/// so every effect it emits is a read.
///
/// # Panics
/// If the classification disagrees with the shard's bytes.
pub fn check_contents(pre_data: &ShardData, descriptor: &TokenDescriptor, contents: AtaContents) {
    assert_eq!(
        ata_core::classify(pre_data, descriptor),
        contents,
        "Associated token account does not hold what the instruction claims"
    );
}

/// Guard on the definition's shard: the declared `kind` must be the definition's.
///
/// Only needed on the `Intended` branch, which emits no chained call and so has
/// nothing downstream to pin `kind` against. The other branches leave that to
/// `InitializeAccount`'s own `CheckHoldingKind` guard on this same shard.
///
/// # Panics
/// If the definition does not describe holdings of `kind`.
pub fn check_definition_kind(pre_data: &ShardData, kind: TokenKind) {
    let definition =
        TokenDefinition::try_from(pre_data).expect("Token Definition account must be valid");
    assert_eq!(
        TokenKind::from_definition(&definition),
        kind,
        "Token Definition does not initialize this Token Holding kind"
    );
}
