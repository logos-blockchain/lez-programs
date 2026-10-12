use std::collections::BTreeMap;

use lee_core::account::{AccountId, ShardData};
use serde::Deserialize;

/// One account read, as the module's C++ hands it over.
///
/// `id` and `status` are the module's own envelope — `readPublicAccount` builds
/// them; `account` is what `lez_core` returned.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AccountRead {
    pub id: String,
    pub status: String,
    #[serde(default)]
    pub account: Option<WalletAccount>,
}

/// The account object `lez_core` emits.
///
/// Mirrors [`lee_core::account::Account`]'s own shape — `nonce` plus a
/// `data.shards` map keyed by program account id — in the hex encoding `lez_core`
/// already uses for everything it hands out. The shape is mirrored but the
/// encoding is not a serde dump of the chain type: `ffiAccountToJson` builds this
/// object by running `bytesToHex` over `wallet_ffi`'s C struct, field by field.
///
/// v0.2.5's `{program_owner, balance, nonce, data}` is gone with the flat account
/// itself. Ownership is no longer a property of an address, a balance is just the
/// native token program's shard, and "the data" is whichever shard you asked for.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct WalletAccount {
    pub nonce: String,
    pub data: WalletAccountData,
}

/// An account's program shards: program account id hex → shard data hex.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct WalletAccountData {
    #[serde(default)]
    pub shards: BTreeMap<String, String>,
}

/// Why an account read yielded no shard to decode.
///
/// Three outcomes rather than one string, because the callers map them to three
/// different public error codes and used to tell them apart by comparing error
/// text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum AccountReadError {
    /// The module's envelope says the read itself failed.
    Failed,
    /// A malformed value — bad hex, or the wrong length.
    Malformed,
    /// The read succeeded, but the named program holds no state on this account.
    ///
    /// The successor to v0.2.5's `program_owner != <expected>`: a shard is keyed
    /// by the program that wrote it, so "this program stored nothing here" is the
    /// same claim, asked of the shard map instead of an ownership field.
    NoShard,
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
/// `ProgramId` (`[u32; 8]`, the ImageID); they are now an `AccountId` directly.
/// `From<ProgramId> for AccountId` expands the words little-endian, so a program
/// deployed at the image-id bijection address has the same hex as before.
pub(crate) fn parse_program_id(value: &str) -> Result<AccountId, String> {
    Ok(AccountId::new(parse_hex_32(value, "program id")?))
}

pub(crate) fn account_id_from_hex(value: &str, label: &str) -> Result<AccountId, String> {
    Ok(AccountId::new(parse_hex_32(value, label)?))
}

pub(crate) fn account_id_hex(account_id: AccountId) -> String {
    hex::encode(account_id.into_value())
}

fn parse_hex_bytes(value: &str) -> Result<Vec<u8>, AccountReadError> {
    if !value.len().is_multiple_of(2)
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(AccountReadError::Malformed);
    }
    hex::decode(value).map_err(|_| AccountReadError::Malformed)
}

/// Pulls one program's shard out of an account read.
///
/// Deliberately permissive about shards it was not asked for, so this works
/// against either `lez_core` read: the shard view returns a map of exactly one
/// entry, and the whole-account read returns every shard anyone ever wrote to the
/// address. Both answer the only question a decoder has — what did *this* program
/// store here — and ignoring the rest is what keeps a third party's shard from
/// turning a good read into a failure.
pub(crate) fn decode_shard(
    read: &AccountRead,
    program_account_id: AccountId,
) -> Result<(AccountId, ShardData), AccountReadError> {
    if read.status != "ok" {
        return Err(AccountReadError::Failed);
    }
    let account_id =
        account_id_from_hex(&read.id, "account id").map_err(|_| AccountReadError::Malformed)?;
    let source = read.account.as_ref().ok_or(AccountReadError::Failed)?;
    // Parsed but unused: the nonce is no longer part of any decode, and rejecting
    // a malformed one here keeps the whole object validated rather than half of it.
    let _ = parse_hex_bytes(&source.nonce)?;

    let shard = source
        .data
        .shards
        .get(&account_id_hex(program_account_id))
        .ok_or(AccountReadError::NoShard)?;
    let bytes = parse_hex_bytes(shard)?;
    if bytes.is_empty() {
        // An empty shard is how the chain spells "this program stored nothing",
        // so it is the same answer as an absent key rather than a short read.
        return Err(AccountReadError::NoShard);
    }
    let data = ShardData::try_from(bytes).map_err(|_| AccountReadError::Malformed)?;
    Ok((account_id, data))
}

#[cfg(test)]
pub(crate) fn account_read(
    id: AccountId,
    program_account_id: AccountId,
    data: &ShardData,
) -> AccountRead {
    AccountRead {
        id: account_id_hex(id),
        status: String::from("ok"),
        account: Some(WalletAccount {
            nonce: hex::encode(0_u128.to_le_bytes()),
            data: WalletAccountData {
                shards: BTreeMap::from([(
                    account_id_hex(program_account_id),
                    hex::encode(data.as_ref()),
                )]),
            },
        }),
    }
}
