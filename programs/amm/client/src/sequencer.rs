//! Lossless adapters for raw sequencer account responses.

use std::{error::Error, fmt};

use nssa_core::{
    account::{Account, AccountId, Data, Nonce},
    program::ProgramId,
};
use serde::Deserialize;
use serde_json::Value;

use crate::quote::AccountSnapshot;

/// Failure while decoding a sequencer account response.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SequencerAccountError {
    /// Response was not valid sequencer account JSON.
    InvalidResponse,
    /// Sequencer returned an RPC error.
    RpcError,
    /// Sequencer returned no account result.
    MissingAccount,
    /// Account data exceeds the NSSA account-data limit.
    AccountDataTooLarge,
}

impl SequencerAccountError {
    /// Stable machine-readable error code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidResponse => "invalid_sequencer_response",
            Self::RpcError => "sequencer_account_error",
            Self::MissingAccount => "sequencer_account_missing",
            Self::AccountDataTooLarge => "account_data_too_large",
        }
    }
}

impl fmt::Display for SequencerAccountError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidResponse => "sequencer account response is invalid",
            Self::RpcError => "sequencer returned an account error",
            Self::MissingAccount => "sequencer returned no account",
            Self::AccountDataTooLarge => "sequencer account data is too large",
        })
    }
}

impl Error for SequencerAccountError {}

#[derive(Deserialize)]
struct SequencerEnvelope {
    #[serde(default)]
    result: Option<SequencerAccount>,
    #[serde(default)]
    error: Option<Value>,
}

#[derive(Deserialize)]
struct SequencerAccount {
    program_owner: ProgramId,
    balance: u128,
    data: Vec<u8>,
    nonce: u128,
}

/// Decodes a raw `getAccount` JSON-RPC response without routing integer fields through a
/// JavaScript numeric value.
///
/// `response` must be the original response text. Passing a JSON value already parsed by a host
/// with IEEE-754 numbers can lose balances or nonces above `2^53` before this function sees them.
pub fn account_snapshot_from_sequencer_response(
    account_id: AccountId,
    response: &str,
) -> Result<AccountSnapshot, SequencerAccountError> {
    let envelope: SequencerEnvelope =
        serde_json::from_str(response).map_err(|_| SequencerAccountError::InvalidResponse)?;
    if envelope.error.is_some() {
        return Err(SequencerAccountError::RpcError);
    }
    let account = envelope
        .result
        .ok_or(SequencerAccountError::MissingAccount)?;
    let data =
        Data::try_from(account.data).map_err(|_| SequencerAccountError::AccountDataTooLarge)?;

    Ok(AccountSnapshot::new(
        account_id,
        Account {
            program_owner: account.program_owner,
            balance: account.balance,
            data,
            nonce: Nonce(account.nonce),
        },
    ))
}
