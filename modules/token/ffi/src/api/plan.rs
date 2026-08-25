use lee_core::account::AccountId;
use risc0_binfmt::ProgramBinary;
use serde_json::{json, Value};
use token_core::{Instruction, MetadataStandard, NewTokenDefinition, NewTokenMetadata};

use super::{
    parse_token_program_id,
    request::{
        BurnPlanRequest, CreateFungiblePlanRequest, CreateFungibleWithMetadataPlanRequest,
        CreateNonFungiblePlanRequest, InitializeHoldingPlanRequest, MintPlanRequest,
        MintWithAuthorityPlanRequest, PrintNftPlanRequest, ProgramIdRequest,
        SetAuthorityPlanRequest, SetAuthorityWithAuthorityPlanRequest, TransferPlanRequest,
    },
    TokenApiError, TokenResult,
};
use crate::account::{account_id_from_hex, account_id_hex, program_id_bytes};

pub fn program_id(request: ProgramIdRequest) -> TokenResult {
    let elf = hex::decode(&request.elf).map_err(|_| TokenApiError::new("bad_request"))?;
    let binary = ProgramBinary::decode(&elf).map_err(|_| TokenApiError::new("bad_request"))?;
    let image_id: lee_core::program::ProgramId = binary
        .compute_image_id()
        .map_err(|_| TokenApiError::new("backend_error"))?
        .into();
    let program_id = AccountId::new(program_id_bytes(image_id));
    Ok(json!({
        "programId": hex::encode(program_id.into_value()),
        "programIdBase58": program_id.to_string(),
    }))
}

pub fn create_fungible_plan(request: CreateFungiblePlanRequest) -> TokenResult {
    let program_id = parse_token_program_id(&request.token_program_id)?;
    let definition_target =
        parse_account_id(&request.definition_target_id, "definition target id")?;
    let holding_target = parse_account_id(&request.holding_target_id, "holding target id")?;
    let total_supply = parse_amount(&request.total_supply_raw)?;
    let mint_authority = parse_authority_sentinel(&request.mint_authority, definition_target)?;
    let instruction = Instruction::NewFungibleDefinition {
        name: request.name,
        total_supply,
        mint_authority,
    };
    plan_response(
        program_id,
        [definition_target, holding_target],
        [true, true],
        instruction,
    )
}

pub fn create_fungible_with_metadata_plan(
    request: CreateFungibleWithMetadataPlanRequest,
) -> TokenResult {
    let program_id = parse_token_program_id(&request.token_program_id)?;
    let definition_target =
        parse_account_id(&request.definition_target_id, "definition target id")?;
    let holding_target = parse_account_id(&request.holding_target_id, "holding target id")?;
    let metadata_target = parse_account_id(&request.metadata_target_id, "metadata target id")?;
    let total_supply = parse_amount(&request.total_supply_raw)?;
    let mint_authority = parse_authority_sentinel(&request.mint_authority, definition_target)?;
    let metadata_standard = parse_metadata_standard(&request.metadata_standard)?;
    let instruction = Instruction::NewDefinitionWithMetadata {
        new_definition: NewTokenDefinition::Fungible {
            name: request.name,
            total_supply,
            mint_authority,
        },
        metadata: Box::new(NewTokenMetadata {
            standard: metadata_standard,
            uri: request.uri,
            creators: request.creators,
        }),
    };
    plan_response(
        program_id,
        [definition_target, holding_target, metadata_target],
        [true, true, true],
        instruction,
    )
}

pub fn create_non_fungible_plan(request: CreateNonFungiblePlanRequest) -> TokenResult {
    let program_id = parse_token_program_id(&request.token_program_id)?;
    let definition_target =
        parse_account_id(&request.definition_target_id, "definition target id")?;
    let master_target = parse_account_id(
        &request.master_holding_target_id,
        "master holding target id",
    )?;
    let metadata_target = parse_account_id(&request.metadata_target_id, "metadata target id")?;
    let printable_supply = parse_amount(&request.printable_supply_raw)?;
    let metadata_standard = parse_metadata_standard(&request.metadata_standard)?;
    let instruction = Instruction::NewDefinitionWithMetadata {
        new_definition: NewTokenDefinition::NonFungible {
            name: request.name,
            printable_supply,
        },
        metadata: Box::new(NewTokenMetadata {
            standard: metadata_standard,
            uri: request.uri,
            creators: request.creators,
        }),
    };
    plan_response(
        program_id,
        [definition_target, master_target, metadata_target],
        [true, true, true],
        instruction,
    )
}

pub fn initialize_holding_plan(request: InitializeHoldingPlanRequest) -> TokenResult {
    let program_id = parse_token_program_id(&request.token_program_id)?;
    let definition_id = parse_account_id(&request.definition_id, "definition id")?;
    let holding_target = parse_account_id(&request.holding_target_id, "holding target id")?;
    plan_response(
        program_id,
        [definition_id, holding_target],
        [false, true],
        Instruction::InitializeAccount,
    )
}

pub fn transfer_plan(request: TransferPlanRequest) -> TokenResult {
    let program_id = parse_token_program_id(&request.token_program_id)?;
    let sender = parse_account_id(&request.sender_holding_id, "sender holding id")?;
    let recipient = parse_account_id(&request.recipient_holding_id, "recipient holding id")?;
    let amount = parse_amount(&request.amount_raw)?;
    plan_response(
        program_id,
        [sender, recipient],
        [true, request.recipient_is_fresh],
        Instruction::Transfer {
            amount_to_transfer: amount,
        },
    )
}

pub fn burn_plan(request: BurnPlanRequest) -> TokenResult {
    let program_id = parse_token_program_id(&request.token_program_id)?;
    let definition = parse_account_id(&request.definition_id, "definition id")?;
    let holding = parse_account_id(&request.holding_id, "holding id")?;
    let amount = parse_amount(&request.amount_raw)?;
    plan_response(
        program_id,
        [definition, holding],
        [false, true],
        Instruction::Burn {
            amount_to_burn: amount,
        },
    )
}

pub fn mint_plan(request: MintPlanRequest) -> TokenResult {
    let program_id = parse_token_program_id(&request.token_program_id)?;
    let definition = parse_account_id(&request.definition_id, "definition id")?;
    let holding = parse_account_id(&request.holding_id, "holding id")?;
    let amount = parse_amount(&request.amount_raw)?;
    plan_response(
        program_id,
        [definition, holding],
        [true, request.holding_is_fresh],
        Instruction::Mint {
            amount_to_mint: amount,
        },
    )
}

pub fn mint_with_authority_plan(request: MintWithAuthorityPlanRequest) -> TokenResult {
    let program_id = parse_token_program_id(&request.token_program_id)?;
    let definition = parse_account_id(&request.definition_id, "definition id")?;
    let holding = parse_account_id(&request.holding_id, "holding id")?;
    let authority = parse_account_id(&request.authority_id, "authority id")?;
    reject_zero_account_id(authority, "invalid_authority")?;
    let amount = parse_amount(&request.amount_raw)?;
    plan_response(
        program_id,
        [definition, holding, authority],
        [false, request.holding_is_fresh, true],
        Instruction::MintWithAuthority {
            amount_to_mint: amount,
        },
    )
}

pub fn set_authority_plan(request: SetAuthorityPlanRequest) -> TokenResult {
    let program_id = parse_token_program_id(&request.token_program_id)?;
    let definition = parse_account_id(&request.definition_id, "definition id")?;
    let new_authority = parse_authority_sentinel(&request.new_authority, definition)?;
    plan_response(
        program_id,
        [definition],
        [true],
        Instruction::SetAuthority { new_authority },
    )
}

pub fn set_authority_with_authority_plan(
    request: SetAuthorityWithAuthorityPlanRequest,
) -> TokenResult {
    let program_id = parse_token_program_id(&request.token_program_id)?;
    let definition = parse_account_id(&request.definition_id, "definition id")?;
    let authority = parse_account_id(&request.authority_id, "authority id")?;
    reject_zero_account_id(authority, "invalid_authority")?;
    let new_authority = parse_authority_sentinel(&request.new_authority, definition)?;
    plan_response(
        program_id,
        [definition, authority],
        [false, true],
        Instruction::SetAuthorityWithAuthority { new_authority },
    )
}

pub fn print_nft_plan(request: PrintNftPlanRequest) -> TokenResult {
    let program_id = parse_token_program_id(&request.token_program_id)?;
    let master = parse_account_id(&request.master_holding_id, "master holding id")?;
    let printed = parse_account_id(
        &request.printed_holding_target_id,
        "printed holding target id",
    )?;
    plan_response(
        program_id,
        [master, printed],
        [true, true],
        Instruction::PrintNft,
    )
}

fn parse_account_id(value: &str, label: &str) -> Result<AccountId, TokenApiError> {
    account_id_from_hex(value, label).map_err(|_| TokenApiError::new("invalid_account_id"))
}

fn parse_amount(value: &Value) -> Result<u128, TokenApiError> {
    match value {
        Value::String(raw) => parse_amount_string(raw),
        Value::Number(raw) => raw
            .as_u64()
            .map(u128::from)
            .ok_or_else(|| TokenApiError::new("bad_amount")),
        _ => Err(TokenApiError::new("bad_amount")),
    }
}

fn parse_amount_string(value: &str) -> Result<u128, TokenApiError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(TokenApiError::new("bad_amount"));
    }
    let normalized = if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    }
    .trim();
    if normalized.is_empty() || !normalized.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(TokenApiError::new("bad_amount"));
    }
    normalized
        .parse::<u128>()
        .map_err(|_| TokenApiError::new("bad_amount"))
}

fn parse_metadata_standard(value: &str) -> Result<MetadataStandard, TokenApiError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "simple" => Ok(MetadataStandard::Simple),
        "expanded" => Ok(MetadataStandard::Expanded),
        _ => Err(TokenApiError::new("invalid_metadata_standard")),
    }
}

fn parse_authority_sentinel(
    value: &str,
    self_id: AccountId,
) -> Result<Option<AccountId>, TokenApiError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(TokenApiError::new("invalid_authority"));
    }
    if trimmed.eq_ignore_ascii_case("none") {
        return Ok(None);
    }
    if trimmed.eq_ignore_ascii_case("self") {
        reject_zero_account_id(self_id, "invalid_authority")?;
        return Ok(Some(self_id));
    }
    let authority = parse_account_id(trimmed, "authority id")
        .map_err(|_| TokenApiError::new("invalid_authority"))?;
    reject_zero_account_id(authority, "invalid_authority")?;
    Ok(Some(authority))
}

fn reject_zero_account_id(account_id: AccountId, code: &'static str) -> Result<(), TokenApiError> {
    if account_id.value() == &[0_u8; 32] {
        return Err(TokenApiError::new(code));
    }
    Ok(())
}

fn plan_response<const N: usize>(
    program_id: lee_core::program::ProgramId,
    account_ids: [AccountId; N],
    signing_requirements: [bool; N],
    instruction: Instruction,
) -> TokenResult {
    let instruction =
        risc0_zkvm::serde::to_vec(&instruction).map_err(|_| TokenApiError::new("backend_error"))?;
    Ok(json!({
        "programId": hex::encode(program_id_bytes(program_id)),
        "accountIds": account_ids.into_iter().map(account_id_hex).collect::<Vec<_>>(),
        "signingRequirements": signing_requirements.into_iter().collect::<Vec<_>>(),
        "instruction": instruction,
    }))
}
