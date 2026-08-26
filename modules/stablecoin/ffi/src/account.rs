use lee_core::{
    account::{Account, AccountId, Data, Nonce},
    program::ProgramId,
};
use serde::Deserialize;

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

pub(crate) fn program_id_bytes(program_id: ProgramId) -> [u8; 32] {
    let mut bytes = [0_u8; 32];
    for (chunk, word) in bytes.chunks_exact_mut(4).zip(program_id) {
        chunk.copy_from_slice(&word.to_le_bytes());
    }
    bytes
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

#[cfg(test)]
pub(crate) fn account_read(id: AccountId, account: &Account) -> AccountRead {
    AccountRead {
        id: account_id_hex(id),
        status: String::from("ok"),
        account: Some(WalletAccount {
            program_owner: hex::encode(program_id_bytes(account.program_owner)),
            balance: hex::encode(account.balance.to_le_bytes()),
            nonce: hex::encode(account.nonce.0.to_le_bytes()),
            data: hex::encode(account.data.as_ref()),
        }),
    }
}
