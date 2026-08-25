use lee_core::{
    account::{Account, AccountId},
    program::ProgramId,
};
use serde_json::{json, Value};
use token_core::{MetadataStandard, TokenDefinition, TokenHolding, TokenMetadata};

use super::{
    parse_token_program_id,
    request::{
        DecodeAccountRequest, DecodeDefinitionRequest, DecodeHoldingRequest, DecodeMetadataRequest,
    },
    TokenApiError, TokenResult,
};
use crate::account::{account_id_hex, decode_account as decode_wallet_account};

pub fn decode_definition(request: DecodeDefinitionRequest) -> TokenResult {
    let token_program_id = parse_token_program_id(&request.token_program_id)?;
    let (account_id, account) = parse_and_validate_account(&request.definition, token_program_id)?;
    let definition = TokenDefinition::try_from(&account.data)
        .map_err(|_| TokenApiError::new("invalid_definition_data"))?;
    Ok(definition_json(account_id, &definition))
}

pub fn decode_holding(request: DecodeHoldingRequest) -> TokenResult {
    let token_program_id = parse_token_program_id(&request.token_program_id)?;
    let (account_id, account) = parse_and_validate_account(&request.holding, token_program_id)?;
    let holding = TokenHolding::try_from(&account.data)
        .map_err(|_| TokenApiError::new("invalid_holding_data"))?;
    Ok(holding_json(account_id, &holding))
}

pub fn decode_metadata(request: DecodeMetadataRequest) -> TokenResult {
    let token_program_id = parse_token_program_id(&request.token_program_id)?;
    let (account_id, account) = parse_and_validate_account(&request.metadata, token_program_id)?;
    let metadata = TokenMetadata::try_from(&account.data)
        .map_err(|_| TokenApiError::new("invalid_metadata_data"))?;
    Ok(metadata_json(account_id, &metadata))
}

pub fn decode_account(request: DecodeAccountRequest) -> TokenResult {
    let token_program_id = parse_token_program_id(&request.token_program_id)?;
    let (account_id, account) = parse_and_validate_account(&request.account, token_program_id)?;

    let definition = TokenDefinition::try_from(&account.data).ok();
    let holding = TokenHolding::try_from(&account.data).ok();
    let metadata = TokenMetadata::try_from(&account.data).ok();
    let matches = usize::from(definition.is_some())
        + usize::from(holding.is_some())
        + usize::from(metadata.is_some());

    match (matches, definition, holding, metadata) {
        (1, Some(value), None, None) => Ok(definition_json(account_id, &value)),
        (1, None, Some(value), None) => Ok(holding_json(account_id, &value)),
        (1, None, None, Some(value)) => Ok(metadata_json(account_id, &value)),
        (0, None, None, None) => Err(TokenApiError::new("invalid_account_data")),
        _ => Err(TokenApiError::new("ambiguous_account_type")),
    }
}

fn parse_and_validate_account(
    read: &crate::account::AccountRead,
    token_program_id: ProgramId,
) -> Result<(AccountId, Account), TokenApiError> {
    let (account_id, account) = decode_wallet_account(read).map_err(map_account_read_error)?;
    if account.program_owner != token_program_id {
        return Err(TokenApiError::new("token_program_mismatch"));
    }
    Ok((account_id, account))
}

fn map_account_read_error(error: String) -> TokenApiError {
    if error == "account read failed" {
        TokenApiError::new("account_read_failed")
    } else {
        TokenApiError::new("bad_request")
    }
}

fn definition_json(account_id: AccountId, definition: &TokenDefinition) -> Value {
    let account_hex = account_id_hex(account_id);
    match definition {
        TokenDefinition::Fungible {
            name,
            total_supply,
            metadata_id,
            authority,
        } => json!({
            "accountType": "definition",
            "kind": "fungible",
            "accountId": account_id.to_string(),
            "accountIdHex": account_hex,
            "name": name,
            "totalSupplyRaw": total_supply.to_string(),
            "metadataId": metadata_id.map(|value| value.to_string()),
            "metadataIdHex": metadata_id.map(account_id_hex),
            "mintAuthorityId": authority.map(|value| value.to_string()),
            "mintAuthorityIdHex": authority.map(account_id_hex),
        }),
        TokenDefinition::NonFungible {
            name,
            printable_supply,
            metadata_id,
        } => json!({
            "accountType": "definition",
            "kind": "nonFungible",
            "accountId": account_id.to_string(),
            "accountIdHex": account_hex,
            "name": name,
            "printableSupplyRaw": printable_supply.to_string(),
            "metadataId": metadata_id.to_string(),
            "metadataIdHex": account_id_hex(*metadata_id),
        }),
    }
}

fn holding_json(account_id: AccountId, holding: &TokenHolding) -> Value {
    let account_hex = account_id_hex(account_id);
    match holding {
        TokenHolding::Fungible {
            definition_id,
            balance,
        } => json!({
            "accountType": "holding",
            "kind": "fungible",
            "accountId": account_id.to_string(),
            "accountIdHex": account_hex,
            "definitionId": definition_id.to_string(),
            "definitionIdHex": account_id_hex(*definition_id),
            "balanceRaw": balance.to_string(),
        }),
        TokenHolding::NftMaster {
            definition_id,
            print_balance,
        } => json!({
            "accountType": "holding",
            "kind": "nftMaster",
            "accountId": account_id.to_string(),
            "accountIdHex": account_hex,
            "definitionId": definition_id.to_string(),
            "definitionIdHex": account_id_hex(*definition_id),
            "printBalanceRaw": print_balance.to_string(),
        }),
        TokenHolding::NftPrintedCopy {
            definition_id,
            owned,
        } => json!({
            "accountType": "holding",
            "kind": "nftPrintedCopy",
            "accountId": account_id.to_string(),
            "accountIdHex": account_hex,
            "definitionId": definition_id.to_string(),
            "definitionIdHex": account_id_hex(*definition_id),
            "owned": owned,
        }),
    }
}

fn metadata_json(account_id: AccountId, metadata: &TokenMetadata) -> Value {
    json!({
        "accountType": "metadata",
        "accountId": account_id.to_string(),
        "accountIdHex": account_id_hex(account_id),
        "definitionId": metadata.definition_id.to_string(),
        "definitionIdHex": account_id_hex(metadata.definition_id),
        "standard": metadata_standard_name(&metadata.standard),
        "uri": metadata.uri,
        "creators": metadata.creators,
        "primarySaleDateRaw": metadata.primary_sale_date.to_string(),
    })
}

fn metadata_standard_name(value: &MetadataStandard) -> &'static str {
    match value {
        MetadataStandard::Simple => "simple",
        MetadataStandard::Expanded => "expanded",
    }
}
