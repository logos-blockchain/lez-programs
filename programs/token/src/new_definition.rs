use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, Data},
    program::{AccountPostState, Claim},
};
use token_core::{
    NewTokenDefinition, NewTokenMetadata, TokenDefinition, TokenHolding, TokenMetadata,
};

/// Validate the mint authority for a freshly created fungible definition.
///
/// `Some(id)` makes the token mintable by `id`; `None` fixes the supply.
/// An all-zero authority id is rejected as it cannot be a real signer.
fn validate_mint_authority(mint_authority: Option<AccountId>) -> Option<AccountId> {
    if let Some(id) = &mint_authority {
        assert!(
            id.value() != &[0u8; 32],
            "Mint authority must be a valid non-zero account ID"
        );
    }
    mint_authority
}

pub fn new_fungible_definition(
    definition_target_account: AccountWithMetadata,
    holding_target_account: AccountWithMetadata,
    name: String,
    total_supply: u128,
    mint_authority: Option<AccountId>,
) -> Vec<AccountPostState> {
    assert_eq!(
        definition_target_account.account,
        Account::default(),
        "Definition target account must have default values"
    );

    assert_eq!(
        holding_target_account.account,
        Account::default(),
        "Holding target account must have default values"
    );
    assert!(
        definition_target_account.is_authorized,
        "Definition target account must be authorized"
    );
    assert!(
        holding_target_account.is_authorized,
        "Holding target account must be authorized"
    );

    let token_definition = TokenDefinition::Fungible {
        name,
        total_supply,
        metadata_id: None,
        authority: validate_mint_authority(mint_authority),
    };
    let token_holding = TokenHolding::Fungible {
        definition_id: definition_target_account.account_id,
        balance: total_supply,
    };

    let mut definition_target_account_post = definition_target_account.account;
    definition_target_account_post.data = Data::from(&token_definition);

    let mut holding_target_account_post = holding_target_account.account;
    holding_target_account_post.data = Data::from(&token_holding);

    vec![
        AccountPostState::new_claimed(definition_target_account_post, Claim::Authorized),
        AccountPostState::new_claimed(holding_target_account_post, Claim::Authorized),
    ]
}

pub fn new_definition_with_metadata(
    definition_target_account: AccountWithMetadata,
    holding_target_account: AccountWithMetadata,
    metadata_target_account: AccountWithMetadata,
    new_definition: NewTokenDefinition,
    metadata: NewTokenMetadata,
) -> Vec<AccountPostState> {
    assert_eq!(
        definition_target_account.account,
        Account::default(),
        "Definition target account must have default values"
    );

    assert_eq!(
        holding_target_account.account,
        Account::default(),
        "Holding target account must have default values"
    );

    assert_eq!(
        metadata_target_account.account,
        Account::default(),
        "Metadata target account must have default values"
    );
    assert!(
        definition_target_account.is_authorized,
        "Definition target account must be authorized"
    );
    assert!(
        holding_target_account.is_authorized,
        "Holding target account must be authorized"
    );
    assert!(
        metadata_target_account.is_authorized,
        "Metadata target account must be authorized"
    );

    let (token_definition, token_holding) = match new_definition {
        NewTokenDefinition::Fungible {
            name,
            total_supply,
            mint_authority,
        } => (
            TokenDefinition::Fungible {
                name,
                total_supply,
                metadata_id: Some(metadata_target_account.account_id),
                authority: validate_mint_authority(mint_authority),
            },
            TokenHolding::Fungible {
                definition_id: definition_target_account.account_id,
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
                metadata_id: metadata_target_account.account_id,
            },
            TokenHolding::NftMaster {
                definition_id: definition_target_account.account_id,
                print_balance: printable_supply,
            },
        ),
    };

    let token_metadata = TokenMetadata {
        definition_id: definition_target_account.account_id,
        standard: metadata.standard,
        uri: metadata.uri,
        creators: metadata.creators,
        primary_sale_date: 0u64,
    };

    let mut definition_target_account_post = definition_target_account.account.clone();
    definition_target_account_post.data = Data::from(&token_definition);

    let mut holding_target_account_post = holding_target_account.account.clone();
    holding_target_account_post.data = Data::from(&token_holding);

    let mut metadata_target_account_post = metadata_target_account.account.clone();
    metadata_target_account_post.data = Data::from(&token_metadata);

    vec![
        AccountPostState::new_claimed(definition_target_account_post, Claim::Authorized),
        AccountPostState::new_claimed(holding_target_account_post, Claim::Authorized),
        AccountPostState::new_claimed(metadata_target_account_post, Claim::Authorized),
    ]
}
