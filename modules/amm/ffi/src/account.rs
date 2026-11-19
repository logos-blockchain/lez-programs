use std::str::FromStr;

use lee_core::account::{Account, AccountId, ActorState, Nonce};
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AccountRead {
    pub id: String,
    #[serde(default)]
    pub program_id: String,
    pub status: String,
    #[serde(default)]
    pub account: Option<WalletAccount>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct WalletAccount {
    pub nonce: String,
    pub actor_states: Vec<WalletActorState>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct WalletActorState {
    pub program: String,
    pub data: String,
}

pub(crate) fn decode_config(bytes: &[u8]) -> Result<amm_core::AmmConfig, String> {
    match borsh::from_slice::<amm_core::AmmState>(bytes) {
        Ok(amm_core::AmmState::Config(config)) => Ok(config),
        _ => Err("invalid_config".into()),
    }
}

pub(crate) fn decode_pool(bytes: &[u8]) -> Result<amm_core::PoolDefinition, String> {
    match borsh::from_slice::<amm_core::AmmState>(bytes) {
        Ok(amm_core::AmmState::Pool(pool)) => Ok(pool),
        _ => Err("no_pool".into()),
    }
}

pub(crate) fn actor_bytes(read: &AccountRead) -> Result<Vec<u8>, String> {
    let (_, account) = decode_account(read)?;
    let program = parse_program_id(&read.program_id)?;
    Ok(account.data.actor_state(program).as_ref().to_vec())
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

/// Parses a program's account id -- the id of its deployed `ProgramHeader` account.
///
/// The wire format is unchanged: 64 lowercase hex characters, the same 32 bytes
/// callers already pass. Before LEZ v0.2.5 those bytes were reassembled into a
/// `AccountId` (`[u32; 8]`, the ImageID); they are now an `AccountId` directly.
/// `From<AccountId> for AccountId` expands the words little-endian, so a program
/// deployed at the image-id bijection address has the same hex as before.
pub(crate) fn parse_program_id(value: &str) -> Result<AccountId, String> {
    Ok(AccountId::new(parse_hex_32(value, "program id")?))
}

pub(crate) fn program_id_base58(program_id: AccountId) -> String {
    program_id.to_string()
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

pub(crate) fn decode_account(read: &AccountRead) -> Result<(AccountId, Account), String> {
    if read.status != "ok" {
        return Err(String::from("account read failed"));
    }
    let account_id = account_id_from_hex(&read.id, "account id")?;
    let source = read
        .account
        .as_ref()
        .ok_or_else(|| String::from("successful account read has no account"))?;
    let mut account = Account {
        nonce: Nonce(
            source
                .nonce
                .parse::<u128>()
                .map_err(|_| "invalid account nonce")?,
        ),
        ..Account::default()
    };
    for actor in &source.actor_states {
        let program = parse_program_id(&actor.program)?;
        if account.data.actor_states.contains_key(&program) {
            return Err("duplicate actor state".into());
        }
        let bytes = hex::decode(&actor.data).map_err(|_| "invalid actor data")?;
        let state = ActorState::from(bytes);
        account.data.set_actor_state(program, state);
    }
    Ok((account_id, account))
}

#[cfg(test)]
pub(crate) fn account_read(id: AccountId, account: &Account) -> AccountRead {
    AccountRead {
        id: account_id_hex(id),
        program_id: account
            .data
            .actor_states
            .keys()
            .next()
            .copied()
            .map(account_id_hex)
            .unwrap_or_default(),
        status: "ok".into(),
        account: Some(WalletAccount {
            nonce: account.nonce.0.to_string(),
            actor_states: account
                .data
                .actor_states
                .iter()
                .map(|(program, data)| WalletActorState {
                    program: account_id_hex(*program),
                    data: hex::encode(data.as_ref()),
                })
                .collect(),
        }),
    }
}
