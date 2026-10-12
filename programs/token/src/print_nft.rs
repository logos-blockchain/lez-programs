use lee_core::{
    account::{AccountId, ShardData},
    program::AccountMeta,
};
use token_core::TokenHolding;

/// Plan half: only the master's authorization is checkable before any read.
///
/// The printed copy needs no authorization: its shard is empty, and writing an
/// empty shard denies that address to nobody.
///
/// # Panics
/// If the master account is not authorized.
pub fn authorize(master_account: &AccountMeta) {
    assert!(
        master_account.is_authorized,
        "Master NFT Account must be authorized"
    );
}

/// Master-side effect: spend one print and pin the collection identity.
///
/// The printed copy's `definition_id` comes from the master's shard, which the
/// copy's own apply never sees — so the master's apply is what makes the declared
/// id true.
///
/// # Panics
/// If the shard is not an NFT master, names a different definition, or has no
/// prints left.
#[must_use]
pub fn print_copy(pre_data: &ShardData, definition_id: AccountId) -> ShardData {
    let mut master_account_data =
        TokenHolding::try_from(pre_data).expect("Invalid Token Holding data");

    let TokenHolding::NftMaster {
        definition_id: master_definition_id,
        print_balance,
    } = &mut master_account_data
    else {
        panic!("Invalid Token Holding provided as NFT Master Account");
    };

    assert_eq!(
        *master_definition_id, definition_id,
        "Printed copy does not belong to the master's Token Definition"
    );

    assert!(
        *print_balance > 1,
        "Insufficient balance to print another NFT copy"
    );
    *print_balance = print_balance
        .checked_sub(1)
        .expect("print balance must be greater than one after validation");

    ShardData::from(&master_account_data)
}
