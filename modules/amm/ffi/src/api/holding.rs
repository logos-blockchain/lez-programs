use lee_core::account::AccountId;
use token_core::{TokenHolding, TokenState};

use crate::account::{decode_account, AccountRead};

#[derive(Clone)]
pub(super) struct SelectedHolding {
    pub(super) id: AccountId,
    pub(super) definition_id: AccountId,
    pub(super) balance: u128,
}

pub(super) fn wallet_holdings(
    reads: &[AccountRead],
    token_program: AccountId,
) -> Vec<SelectedHolding> {
    reads
        .iter()
        .filter_map(|read| decode_fungible_holding(read, token_program).ok())
        .collect()
}

pub(super) fn decode_fungible_holding(
    read: &AccountRead,
    token_program: AccountId,
) -> Result<SelectedHolding, String> {
    let (id, account) = decode_account(read)?;
    let TokenState::Holding(TokenHolding {
        definition_id,
        balance,
    }) = borsh::from_slice(account.data.actor_state(token_program).as_ref())
        .map_err(|_| "invalid fungible holding")?
    else {
        return Err("invalid fungible holding".into());
    };
    Ok(SelectedHolding {
        id,
        definition_id,
        balance,
    })
}

pub(super) fn select_holding(
    holdings: &[SelectedHolding],
    definition_id: AccountId,
) -> Option<SelectedHolding> {
    holdings
        .iter()
        .filter(|holding| holding.definition_id == definition_id)
        .max_by(|left, right| {
            left.balance
                .cmp(&right.balance)
                .then_with(|| right.id.cmp(&left.id))
        })
        .cloned()
}
