use lee_core::{
    account::{Account, AccountWithMetadata, Data},
    program::{AccountPostState, Claim},
};
use program_revert::UnwrapOrRevert as _;
use token_core::{error, TokenHolding};

pub fn transfer(
    sender: AccountWithMetadata,
    recipient: AccountWithMetadata,
    balance_to_move: u128,
) -> Vec<AccountPostState> {
    program_revert::require!(
        error::INVALID_INPUT,
        sender.is_authorized,
        "Sender authorization is missing"
    );

    let mut sender_holding = TokenHolding::try_from(&sender.account.data)
        .unwrap_or_revert(error::INVALID_INPUT, "Invalid sender data");

    let mut recipient_holding = if recipient.account == Account::default() {
        TokenHolding::zeroized_clone_from(&sender_holding)
    } else {
        TokenHolding::try_from(&recipient.account.data)
            .unwrap_or_revert(error::INVALID_INPUT, "Invalid recipient data")
    };

    program_revert::require_eq!(
        error::INVALID_INPUT,
        sender_holding.definition_id(),
        recipient_holding.definition_id(),
        "Sender and recipient definition id mismatch"
    );

    match (&mut sender_holding, &mut recipient_holding) {
        (
            TokenHolding::Fungible {
                definition_id: _,
                balance: sender_balance,
            },
            TokenHolding::Fungible {
                definition_id: _,
                balance: recipient_balance,
            },
        ) => {
            *sender_balance = sender_balance
                .checked_sub(balance_to_move)
                .unwrap_or_revert(error::INSUFFICIENT_BALANCE, "Insufficient balance");

            *recipient_balance = recipient_balance
                .checked_add(balance_to_move)
                .unwrap_or_revert(error::ARITHMETIC, "Recipient balance overflow");
        }
        (
            TokenHolding::NftMaster {
                definition_id: _,
                print_balance: sender_print_balance,
            },
            TokenHolding::NftMaster {
                definition_id: _,
                print_balance: recipient_print_balance,
            },
        ) => {
            program_revert::require_eq!(
                error::INVALID_INPUT,
                *recipient_print_balance,
                0,
                "Invalid balance in recipient account for NFT transfer"
            );

            program_revert::require_eq!(
                error::INVALID_INPUT,
                *sender_print_balance,
                balance_to_move,
                "Invalid balance for NFT Master transfer"
            );

            std::mem::swap(sender_print_balance, recipient_print_balance);
        }
        (
            TokenHolding::NftPrintedCopy {
                definition_id: _,
                owned: sender_owned,
            },
            TokenHolding::NftPrintedCopy {
                definition_id: _,
                owned: recipient_owned,
            },
        ) => {
            program_revert::require_eq!(
                error::INVALID_INPUT,
                balance_to_move,
                1,
                "Invalid balance for NFT Printed Copy transfer"
            );

            program_revert::require!(
                error::INVALID_INPUT,
                *sender_owned,
                "Sender does not own the NFT Printed Copy"
            );

            program_revert::require!(
                error::INVALID_INPUT,
                !*recipient_owned,
                "Recipient already owns the NFT Printed Copy"
            );

            *sender_owned = false;
            *recipient_owned = true;
        }
        _ => {
            program_revert::revert!(
                error::INVALID_INPUT,
                "Mismatched token holding types for transfer"
            );
        }
    };

    let mut sender_post = sender.account;
    sender_post.data = Data::from(&sender_holding);

    let mut recipient_post = recipient.account;
    recipient_post.data = Data::from(&recipient_holding);

    vec![
        AccountPostState::new(sender_post),
        AccountPostState::new_claimed_if_default(recipient_post, Claim::Authorized),
    ]
}
