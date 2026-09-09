use lee_core::{
    account::{AccountWithMetadata, Data},
    program::AccountPostState,
};
use program_revert::UnwrapOrRevert as _;
use token_core::{error, TokenDefinition, TokenHolding};

pub fn burn(
    definition_account: AccountWithMetadata,
    user_holding_account: AccountWithMetadata,
    amount_to_burn: u128,
) -> Vec<AccountPostState> {
    program_revert::require!(
        error::INVALID_INPUT,
        user_holding_account.is_authorized,
        "Authorization is missing"
    );

    let mut definition = TokenDefinition::try_from(&definition_account.account.data)
        .unwrap_or_revert(
            error::INVALID_INPUT,
            "Token Definition account must be valid",
        );
    let mut holding = TokenHolding::try_from(&user_holding_account.account.data)
        .unwrap_or_revert(error::INVALID_INPUT, "Token Holding account must be valid");

    program_revert::require_eq!(
        error::INVALID_INPUT,
        definition_account.account_id,
        holding.definition_id(),
        "Mismatch Token Definition and Token Holding"
    );

    match (&mut definition, &mut holding) {
        (
            TokenDefinition::Fungible {
                name: _,
                metadata_id: _,
                total_supply,
                authority: _,
            },
            TokenHolding::Fungible {
                definition_id: _,
                balance,
            },
        ) => {
            *balance = balance
                .checked_sub(amount_to_burn)
                .unwrap_or_revert(error::INSUFFICIENT_BALANCE, "Insufficient balance to burn");

            *total_supply = total_supply
                .checked_sub(amount_to_burn)
                .unwrap_or_revert(error::ARITHMETIC, "Total supply underflow");
        }
        (
            TokenDefinition::NonFungible {
                name: _,
                printable_supply,
                metadata_id: _,
            },
            TokenHolding::NftMaster {
                definition_id: _,
                print_balance,
            },
        ) => {
            *printable_supply = printable_supply
                .checked_sub(amount_to_burn)
                .unwrap_or_revert(error::ARITHMETIC, "Printable supply underflow");

            *print_balance = print_balance
                .checked_sub(amount_to_burn)
                .unwrap_or_revert(error::INSUFFICIENT_BALANCE, "Insufficient balance to burn");
        }
        (
            TokenDefinition::NonFungible {
                name: _,
                printable_supply,
                metadata_id: _,
            },
            TokenHolding::NftPrintedCopy {
                definition_id: _,
                owned,
            },
        ) => {
            program_revert::require_eq!(
                error::INVALID_INPUT,
                amount_to_burn,
                1,
                "Invalid balance to burn for NFT Printed Copy"
            );

            program_revert::require!(
                error::INVALID_INPUT,
                *owned,
                "Cannot burn unowned NFT Printed Copy"
            );

            *printable_supply = printable_supply
                .checked_sub(1)
                .unwrap_or_revert(error::ARITHMETIC, "Printable supply underflow");

            *owned = false;
        }
        _ => program_revert::revert!(
            error::INVALID_INPUT,
            "Mismatched Token Definition and Token Holding types"
        ),
    }

    let mut definition_post = definition_account.account;
    definition_post.data = Data::from(&definition);

    let mut holding_post = user_holding_account.account;
    holding_post.data = Data::from(&holding);

    vec![
        AccountPostState::new(definition_post),
        AccountPostState::new(holding_post),
    ]
}
