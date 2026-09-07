use lee_core::{
    account::{Account, AccountWithMetadata, Data},
    program::{AccountPostState, Claim},
};
use program_revert::UnwrapOrRevert as _;
use token_core::{error, TokenHolding};

pub fn print_nft(
    master_account: AccountWithMetadata,
    printed_account: AccountWithMetadata,
) -> Vec<AccountPostState> {
    program_revert::require!(
        error::INVALID_INPUT,
        master_account.is_authorized,
        "Master NFT Account must be authorized"
    );

    program_revert::require_eq!(
        error::INVALID_INPUT,
        printed_account.account,
        Account::default(),
        "Printed Account must be uninitialized"
    );
    program_revert::require!(
        error::INVALID_INPUT,
        printed_account.is_authorized,
        "Printed Account must be authorized"
    );

    let mut master_account_data = TokenHolding::try_from(&master_account.account.data)
        .unwrap_or_revert(error::INVALID_INPUT, "Invalid Token Holding data");

    let TokenHolding::NftMaster {
        definition_id,
        print_balance,
    } = &mut master_account_data
    else {
        program_revert::revert!(
            error::INVALID_INPUT,
            "Invalid Token Holding provided as NFT Master Account"
        );
    };

    let definition_id = *definition_id;

    program_revert::require!(
        error::INSUFFICIENT_BALANCE,
        *print_balance > 1,
        "Insufficient balance to print another NFT copy"
    );
    *print_balance = print_balance
        .checked_sub(1)
        .expect("print balance must be greater than one after validation");

    let mut master_account_post = master_account.account;
    master_account_post.data = Data::from(&master_account_data);

    let mut printed_account_post = printed_account.account;
    printed_account_post.data = Data::from(&TokenHolding::NftPrintedCopy {
        definition_id,
        owned: true,
    });

    vec![
        AccountPostState::new(master_account_post),
        AccountPostState::new_claimed(printed_account_post, Claim::Authorized),
    ]
}
