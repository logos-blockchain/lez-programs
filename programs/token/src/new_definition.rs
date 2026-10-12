use lee_core::{
    account::{AccountId, ShardData},
    program::AccountMeta,
};
use token_core::{
    NewTokenDefinition, NewTokenMetadata, TokenDefinition, TokenHolding, TokenMetadata,
};

/// Validate the mint authority for a freshly created fungible definition.
///
/// `Some(id)` makes the token mintable by `id`; `None` fixes the supply.
/// An all-zero authority id is rejected as it cannot be a real signer.
///
/// # Panics
/// If `mint_authority` is the all-zero id.
fn validate_mint_authority(mint_authority: Option<AccountId>) -> Option<AccountId> {
    if let Some(id) = &mint_authority {
        assert!(
            id.value() != &[0u8; 32],
            "Mint authority must be a valid non-zero account ID"
        );
    }
    mint_authority
}

/// Plan half: every target must authorize its own creation.
///
/// These instructions read nothing, so the whole check is here. The emptiness of
/// each target is asserted by [`create`] in the apply phase, which is the only
/// phase that can see it.
///
/// # Panics
/// If any target account is not authorized.
pub fn authorize_targets(targets: &[&AccountMeta]) {
    for target in targets {
        assert!(
            target.is_authorized,
            "Target account must be authorized to be created"
        );
    }
}

/// The definition and holding records for a fungible token with no metadata.
#[must_use]
pub fn fungible_records(
    definition_id: AccountId,
    name: String,
    total_supply: u128,
    mint_authority: Option<AccountId>,
) -> (ShardData, ShardData) {
    let definition = TokenDefinition::Fungible {
        name,
        total_supply,
        metadata_id: None,
        authority: validate_mint_authority(mint_authority),
    };
    let holding = TokenHolding::Fungible {
        definition_id,
        balance: total_supply,
    };
    (ShardData::from(&definition), ShardData::from(&holding))
}

/// The definition, holding and metadata records for a token with metadata.
#[must_use]
pub fn records_with_metadata(
    definition_id: AccountId,
    metadata_id: AccountId,
    new_definition: NewTokenDefinition,
    metadata: NewTokenMetadata,
) -> (ShardData, ShardData, ShardData) {
    let (definition, holding) = match new_definition {
        NewTokenDefinition::Fungible {
            name,
            total_supply,
            mint_authority,
        } => (
            TokenDefinition::Fungible {
                name,
                total_supply,
                metadata_id: Some(metadata_id),
                authority: validate_mint_authority(mint_authority),
            },
            TokenHolding::Fungible {
                definition_id,
                balance: total_supply,
            },
        ),
        NewTokenDefinition::NonFungible {
            name,
            printable_supply,
        } => (
            TokenDefinition::NonFungible {
                name,
                printable_supply,
                metadata_id,
            },
            TokenHolding::NftMaster {
                definition_id,
                print_balance: printable_supply,
            },
        ),
    };

    let token_metadata = TokenMetadata {
        definition_id,
        standard: metadata.standard,
        uri: metadata.uri,
        creators: metadata.creators,
        primary_sale_date: 0u64,
    };

    (
        ShardData::from(&definition),
        ShardData::from(&holding),
        ShardData::from(&token_metadata),
    )
}

/// Write a whole record into a shard that must be empty.
///
/// The emptiness assert is what `#[account(init)]` used to express in the plan
/// phase and no longer can: nothing there can see whether a shard holds bytes.
///
/// # Panics
/// If the shard already holds data.
#[must_use]
pub fn create(pre_data: &ShardData, data: ShardData) -> ShardData {
    assert!(
        pre_data.is_empty(),
        "Target account must not already hold data"
    );
    data
}
