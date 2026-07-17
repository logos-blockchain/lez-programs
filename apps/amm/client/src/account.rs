use std::str::FromStr;

use nssa_core::{
    account::{Account, AccountId, Data, Nonce},
    program::ProgramId,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::api::NormalizeAccountRpcRequest;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AccountRead {
    pub id: String,
    pub status: String,
    #[serde(default)]
    pub account: Option<WalletAccount>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct WalletAccount {
    pub program_owner: String,
    pub balance: String,
    pub nonce: String,
    pub data: String,
}

pub(crate) fn parse_hex_32(value: &str, label: &str) -> Result<[u8; 32], String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!(
            "{label} must be 64 lowercase hexadecimal characters"
        ));
    }

    let mut bytes = [0_u8; 32];
    hex::decode_to_slice(value, &mut bytes).map_err(|error| format!("invalid {label}: {error}"))?;
    Ok(bytes)
}

pub(crate) fn parse_program_id(value: &str) -> Result<ProgramId, String> {
    let bytes = parse_hex_32(value, "program id")?;
    let mut program_id = [0_u32; 8];
    for (word, chunk) in program_id.iter_mut().zip(bytes.chunks_exact(4)) {
        let chunk: [u8; 4] = chunk
            .try_into()
            .map_err(|_| String::from("program id word has invalid length"))?;
        *word = u32::from_le_bytes(chunk);
    }
    Ok(program_id)
}

#[cfg(test)]
pub(crate) fn program_id_hex(program_id: ProgramId) -> String {
    let bytes = program_id
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect::<Vec<_>>();
    hex::encode(bytes)
}

pub(crate) fn program_id_base58(program_id: ProgramId) -> String {
    AccountId::new(program_id_bytes(program_id)).to_string()
}

pub(crate) fn program_id_bytes(program_id: ProgramId) -> [u8; 32] {
    let mut bytes = [0_u8; 32];
    for (chunk, word) in bytes.chunks_exact_mut(4).zip(program_id) {
        chunk.copy_from_slice(&word.to_le_bytes());
    }
    bytes
}

pub(crate) fn parse_base58_id(value: &str, label: &str) -> Result<AccountId, String> {
    AccountId::from_str(value).map_err(|error| format!("invalid {label}: {error}"))
}

pub(crate) fn account_id_from_hex(value: &str, label: &str) -> Result<AccountId, String> {
    Ok(AccountId::new(parse_hex_32(value, label)?))
}

pub(crate) fn account_id_hex(account_id: AccountId) -> String {
    hex::encode(account_id.into_value())
}

fn parse_le_u128(value: &str, label: &str) -> Result<u128, String> {
    if value.len() != 32
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!(
            "{label} must be 32 lowercase hexadecimal characters"
        ));
    }
    let mut bytes = [0_u8; 16];
    hex::decode_to_slice(value, &mut bytes).map_err(|error| format!("invalid {label}: {error}"))?;
    Ok(u128::from_le_bytes(bytes))
}

pub(crate) fn decode_account(read: &AccountRead) -> Result<(AccountId, Account), String> {
    if read.status != "ok" {
        return Err(String::from("account read failed"));
    }
    let account_id = account_id_from_hex(&read.id, "account id")?;
    let source = read
        .account
        .as_ref()
        .ok_or_else(|| String::from("successful account read has no account"))?;
    let program_owner = parse_program_id(&source.program_owner)?;
    let balance = parse_le_u128(&source.balance, "account balance")?;
    let nonce = parse_le_u128(&source.nonce, "account nonce")?;
    if source.data.len() % 2 != 0
        || !source
            .data
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(String::from(
            "account data must be lowercase even-length hexadecimal",
        ));
    }
    let data =
        hex::decode(&source.data).map_err(|error| format!("invalid account data: {error}"))?;
    let data =
        Data::try_from(data).map_err(|error| format!("account data is too large: {error}"))?;

    Ok((
        account_id,
        Account {
            program_owner,
            balance,
            data,
            nonce: Nonce(nonce),
        },
    ))
}

#[derive(Deserialize)]
struct RpcEnvelope {
    #[serde(default)]
    result: Option<RpcAccount>,
    #[serde(default)]
    error: Option<Value>,
}

#[derive(Deserialize)]
struct RpcAccount {
    program_owner: ProgramId,
    balance: u128,
    data: Vec<u8>,
    nonce: u128,
}

pub(crate) fn normalize_account_rpc(request: NormalizeAccountRpcRequest) -> Result<Value, String> {
    parse_hex_32(&request.account_id, "account id")?;
    let envelope: RpcEnvelope = serde_json::from_str(&request.response)
        .map_err(|error| format!("invalid sequencer response: {error}"))?;
    if envelope.error.is_some() {
        return Err(String::from("sequencer returned an account-read error"));
    }
    let account = envelope
        .result
        .ok_or_else(|| String::from("sequencer returned no account"))?;
    let data = Data::try_from(account.data)
        .map_err(|error| format!("account data is too large: {error}"))?;

    Ok(json!({
        "id": request.account_id,
        "status": "ok",
        "account": {
            "program_owner": hex::encode(program_id_bytes(account.program_owner)),
            "balance": hex::encode(account.balance.to_le_bytes()),
            "nonce": hex::encode(account.nonce.to_le_bytes()),
            "data": hex::encode(data.as_ref()),
        },
    }))
}

#[cfg(test)]
pub(crate) fn account_read(id: AccountId, account: &Account) -> AccountRead {
    AccountRead {
        id: account_id_hex(id),
        status: String::from("ok"),
        account: Some(WalletAccount {
            program_owner: program_id_hex(account.program_owner),
            balance: hex::encode(account.balance.to_le_bytes()),
            nonce: hex::encode(account.nonce.0.to_le_bytes()),
            data: hex::encode(account.data.as_ref()),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_rpc_u128_without_precision_loss() {
        let account_id = hex::encode([7_u8; 32]);
        let value = normalize_account_rpc(NormalizeAccountRpcRequest {
            account_id: account_id.clone(),
            response: String::from(
                r#"{"jsonrpc":"2.0","id":1,"result":{"program_owner":[1,2,3,4,5,6,7,8],"balance":340282366920938463463374607431768211455,"data":[0,255],"nonce":18446744073709551617}}"#,
            ),
        })
        .unwrap();

        assert_eq!(value["id"], account_id);
        assert_eq!(
            value["account"]["balance"],
            "ffffffffffffffffffffffffffffffff"
        );
        assert_eq!(
            value["account"]["nonce"],
            "01000000000000000100000000000000"
        );
        assert_eq!(value["account"]["data"], "00ff");
        assert_eq!(
            value["account"]["program_owner"],
            "0100000002000000030000000400000005000000060000000700000008000000"
        );
    }
}
