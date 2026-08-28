use lee_core::account::AccountId;
use serde_json::{json, Value};
use stablecoin_core::{compute_position_pda, compute_position_vault_pda, Position};

use super::{
    parse_stablecoin_program_id, DecodePositionRequest, PositionInfoRequest, StablecoinApiError,
    StablecoinResult,
};
use crate::account::{account_id_from_hex, account_id_hex, decode_account};

pub fn position_info(request: PositionInfoRequest) -> StablecoinResult {
    let stablecoin_program_id = parse_stablecoin_program_id(&request.stablecoin_program_id)?;
    let owner_id = parse_owner_id(&request.owner_id)?;
    let position_nonce = parse_position_nonce(&request.position_nonce)?;
    let position_id = compute_position_pda(stablecoin_program_id, owner_id, position_nonce);
    let vault_id = compute_position_vault_pda(stablecoin_program_id, position_id);

    Ok(position_identity_value(
        owner_id,
        position_nonce,
        position_id,
        vault_id,
    ))
}

pub fn decode_position(request: DecodePositionRequest) -> StablecoinResult {
    let stablecoin_program_id = parse_stablecoin_program_id(&request.stablecoin_program_id)?;
    let owner_id = parse_owner_id(&request.owner_id)?;
    let position_nonce = parse_position_nonce(&request.position_nonce)?;
    let expected_position_id =
        compute_position_pda(stablecoin_program_id, owner_id, position_nonce);
    let expected_vault_id = compute_position_vault_pda(stablecoin_program_id, expected_position_id);
    let (position_id, account) = decode_account(&request.position)
        .map_err(|_| StablecoinApiError::new("account_read_failed"))?;

    if position_id != expected_position_id {
        return Err(StablecoinApiError::new("position_pda_mismatch"));
    }
    if account.program_owner != stablecoin_program_id {
        return Err(StablecoinApiError::new("stablecoin_program_mismatch"));
    }

    let position = Position::try_from(&account.data)
        .map_err(|_| StablecoinApiError::new("invalid_position_data"))?;
    if position.owner_account_id != owner_id {
        return Err(StablecoinApiError::new("position_owner_mismatch"));
    }
    if position.position_nonce != position_nonce {
        return Err(StablecoinApiError::new("position_nonce_mismatch"));
    }
    if position.vault_account_id != expected_vault_id {
        return Err(StablecoinApiError::new("position_vault_mismatch"));
    }

    let mut value = position_identity_value(
        owner_id,
        position_nonce,
        expected_position_id,
        expected_vault_id,
    );
    value["collateralAmount"] = json!(position.collateral_amount.to_string());
    value["normalizedDebtAmount"] = json!(position.normalized_debt_amount.to_string());
    value["openedAt"] = json!(position.opened_at.to_string());
    Ok(value)
}

fn parse_owner_id(value: &str) -> Result<AccountId, StablecoinApiError> {
    let owner_id = account_id_from_hex(value, "owner id")
        .map_err(|_| StablecoinApiError::new("invalid_account_id"))?;
    if owner_id.value() == &[0_u8; 32] {
        return Err(StablecoinApiError::new("invalid_account_id"));
    }
    Ok(owner_id)
}

fn parse_position_nonce(value: &str) -> Result<u64, StablecoinApiError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(StablecoinApiError::new("invalid_numeric_value"));
    }
    value
        .parse::<u64>()
        .map_err(|_| StablecoinApiError::new("invalid_numeric_value"))
}

fn position_identity_value(
    owner_id: AccountId,
    position_nonce: u64,
    position_id: AccountId,
    vault_id: AccountId,
) -> Value {
    json!({
        "ownerId": owner_id.to_string(),
        "ownerIdHex": account_id_hex(owner_id),
        "positionNonce": position_nonce.to_string(),
        "positionId": position_id.to_string(),
        "positionIdHex": account_id_hex(position_id),
        "vaultId": vault_id.to_string(),
        "vaultIdHex": account_id_hex(vault_id),
    })
}
