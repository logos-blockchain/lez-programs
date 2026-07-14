use nssa_core::Commitment;
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};

use super::{
    commitment::{FundingCommitment, QuoteCommitment, SourceCommitment},
    holding::SelectedHolding,
    pair::PairIds,
    quote_error::issue,
};
use crate::account::{decode_account, AccountRead};

pub(super) fn funding_issues(
    wallet_available: bool,
    pair: PairIds,
    holding_a: &Option<SelectedHolding>,
    requested_a: u128,
    holding_b: &Option<SelectedHolding>,
    requested_b: u128,
    fields: [&str; 2],
) -> Vec<Value> {
    if !wallet_available {
        return vec![issue(
            "no_wallet",
            "Connect a wallet to submit.",
            &[],
            json!({}),
        )];
    }
    let mut errors = Vec::new();
    for (token_id, holding, requested, field) in [
        (pair.token_a, holding_a, requested_a, fields[0]),
        (pair.token_b, holding_b, requested_b, fields[1]),
    ] {
        let available = holding.as_ref().map_or(0, |value| value.balance);
        if available < requested {
            errors.push(issue(
                "amount_exceeds_balance",
                "Amount exceeds the selected wallet holding balance.",
                &[field],
                json!({
                    "requestedRaw": requested.to_string(),
                    "availableRaw": available.to_string(),
                    "holdingFound": holding.is_some(),
                    "tokenId": token_id.to_string(),
                }),
            ));
        }
    }
    errors
}

pub(super) fn funding_commitments(
    pair: PairIds,
    holding_a: &Option<SelectedHolding>,
    requested_a: u128,
    holding_b: &Option<SelectedHolding>,
    requested_b: u128,
) -> Vec<FundingCommitment> {
    [
        (pair.token_a, holding_a, requested_a),
        (pair.token_b, holding_b, requested_b),
    ]
    .into_iter()
    .map(|(token_id, holding, requested)| FundingCommitment {
        token_id: token_id.into_value(),
        holding_id: holding.as_ref().map(|value| value.id.into_value()),
        available: holding.as_ref().map_or(0, |value| value.balance),
        requested,
    })
    .collect()
}

pub(super) fn sources_from_reads(
    reads: &[(&str, &AccountRead)],
) -> Result<Vec<SourceCommitment>, String> {
    reads
        .iter()
        .map(|(role, read)| {
            let (id, account) = decode_account(read)?;
            Ok(SourceCommitment {
                role: String::from(*role),
                commitment: Commitment::new(&id, &account).to_byte_array(),
            })
        })
        .collect()
}

pub(super) fn append_holding_source(
    sources: &mut Vec<SourceCommitment>,
    role: &str,
    holding: Option<&SelectedHolding>,
) {
    if let Some(holding) = holding {
        sources.push(SourceCommitment {
            role: String::from(role),
            commitment: Commitment::new(&holding.id, &holding.account).to_byte_array(),
        });
    }
}

pub(super) fn hash_quote(commitment: &QuoteCommitment) -> Result<String, String> {
    let bytes = borsh::to_vec(commitment)
        .map_err(|error| format!("quote commitment serialization failed: {error}"))?;
    Ok(format!("sha256:{}", hex::encode(Sha256::digest(bytes))))
}
