use lee_core::account::AccountId;
use token_core::TokenHolding;

use crate::account::{program_shard, AccountRead};

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
    // Asking for the Token Program's shard is the v0.2.5 `program_owner !=
    // token_program` check: an account it never wrote to holds no shard of its.
    let (id, shard) = program_shard(read, token_program, "token holding")?;
    let TokenHolding::Fungible {
        definition_id,
        balance,
    } = TokenHolding::try_from(&shard).map_err(|_| String::from("invalid fungible holding"))?
    else {
        return Err(String::from("invalid fungible holding"));
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
