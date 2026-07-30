use nssa_core::{
    account::{Account, AccountId},
    program::ProgramId,
};
use token_core::TokenHolding;

use crate::account::{account_id_from_hex, decode_account, parse_base58_id, AccountRead};

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
    requested_id: Option<&str>,
) -> Option<SelectedHolding> {
    let options = holding_options(holdings, definition_id);
    let Some(requested_id) = requested_id else {
        return options.first().cloned();
    };
    let requested_id = account_id_from_hex(requested_id, "holding id")
        .or_else(|_| parse_base58_id(requested_id, "holding id"))
        .ok()?;
    options
        .iter()
        .find(|holding| holding.id == requested_id)
        .cloned()
}

pub(super) fn holding_options(
    holdings: &[SelectedHolding],
    definition_id: AccountId,
) -> Vec<SelectedHolding> {
    let mut options = holdings
        .iter()
        .filter(|holding| holding.definition_id == definition_id)
        .cloned()
        .collect::<Vec<_>>();
    options.sort_by(|left, right| {
        right
            .balance
            .cmp(&left.balance)
            .then_with(|| left.id.cmp(&right.id))
    });
    options
}
