use nssa_core::{
    account::{Account, AccountId},
    program::ProgramId,
};
use token_core::TokenHolding;

use crate::account::{decode_account, AccountRead};

#[derive(Clone)]
pub(super) struct SelectedHolding {
    pub(super) id: AccountId,
    pub(super) definition_id: AccountId,
    pub(super) balance: u128,
    pub(super) account: Account,
}

pub(super) fn wallet_holdings(
    reads: &[AccountRead],
    token_program: ProgramId,
) -> Vec<SelectedHolding> {
    reads
        .iter()
        .filter_map(|read| decode_fungible_holding(read, token_program).ok())
        .collect()
}

pub(super) fn decode_fungible_holding(
    read: &AccountRead,
    token_program: ProgramId,
) -> Result<SelectedHolding, String> {
    let (id, account) = decode_account(read)?;
    if account.program_owner != token_program {
        return Err(String::from("holding owner mismatch"));
    }
    let TokenHolding::Fungible {
        definition_id,
        balance,
    } = TokenHolding::try_from(&account.data)
        .map_err(|_| String::from("invalid fungible holding"))?
    else {
        return Err(String::from("invalid fungible holding"));
    };
    Ok(SelectedHolding {
        id,
        definition_id,
        balance,
        account,
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
